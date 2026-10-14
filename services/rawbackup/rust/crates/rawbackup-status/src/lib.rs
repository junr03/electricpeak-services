#![allow(clippy::module_name_repetitions, clippy::too_many_lines)]

mod camera;
mod internxt;
mod inventory;
mod model;
mod status;
mod support;
mod systemd;
#[cfg(test)]
mod tests;

use camera::camera_inventory;
use internxt::internxt_inventory;
use inventory::{
    external_inventory, raw_directory_inventory, repair_incomplete_external_inventory,
};
use model::{
    BasicInventory, CameraAudit, CameraStatus, ExifRecord, FlowStatus, ImportManifest,
    InternxtLocation, Inventory, InventoryKind, LocalLocation, Locations, PathVerification,
    PipelineFlows, PipelineStatus, RawDirectoryInventory, RemoteInventory, SsdLocation,
    StatusSnapshot, SyncState, UnitStatus,
};
use status::{
    classify_external, classify_internxt_raw, classify_internxt_sidecars, reconciliation_status,
};
use std::env;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use support::write_json_atomic;
use systemd::{active_job_for_kind, import_units, iso_now, unit_status};

type AppResult<T> = Result<T, Box<dyn Error>>;

const STATE_DIR: &str = "/var/lib/rawbackup";
const SD_IMPORT_MANIFEST: &str = "/var/lib/photo-workflow/latest-sd-import.json";
const RECONCILE_STATUS: &str = "/var/lib/rawbackup/reconcile/status.json";
const INTERNXT_RAW_PATH: &str = "internxt:photos/raw";
const INTERNXT_SCOPE: &str = "managed-photo-files-v1";
const AUDIT_MAX_AGE_SECONDS: u64 = 15 * 60;
const RAW_EXTENSIONS: &[&str] = &["arw", "cr2", "cr3", "dng", "nef", "orf", "raf", "rw2"];
const SIDECAR_EXTENSIONS: &[&str] = &["acr", "photo-edit", "xmp"];
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct Config {
    local_root: PathBuf,
    external_device: PathBuf,
    external_subdirectory: PathBuf,
    external_raw_subdirectory: PathBuf,
    internxt_config_path: PathBuf,
    state_dir: PathBuf,
    sd_import_manifest: PathBuf,
    reconcile_status: PathBuf,
}

impl Config {
    fn from_environment() -> AppResult<Self> {
        Ok(Self {
            local_root: required_path("RAWBACKUP_LOCAL_ROOT", false)?,
            external_device: required_path("RAWBACKUP_EXTERNAL_DEVICE", false)?,
            external_subdirectory: required_path("RAWBACKUP_EXTERNAL_SUBDIRECTORY", true)?,
            external_raw_subdirectory: required_path("RAWBACKUP_EXTERNAL_RAW_SUBDIRECTORY", false)?,
            internxt_config_path: required_path("RAWBACKUP_INTERNXT_CONFIG", false)?,
            state_dir: optional_path("RAWBACKUP_STATE_DIR", STATE_DIR)?,
            sd_import_manifest: optional_path("RAWBACKUP_SD_IMPORT_MANIFEST", SD_IMPORT_MANIFEST)?,
            reconcile_status: optional_path("RAWBACKUP_RECONCILE_STATUS", RECONCILE_STATUS)?,
        })
    }

    fn status_path(&self) -> PathBuf {
        self.state_dir.join("status.json")
    }

    fn internxt_audit_path(&self) -> PathBuf {
        self.state_dir.join("internxt-audit.json")
    }

    fn camera_audit_path(&self) -> PathBuf {
        self.state_dir.join("camera-audit.json")
    }
}
/// Collect and atomically publish the Raw Backup dashboard snapshot.
///
/// # Errors
///
/// Returns an error when required configuration is missing, a local inventory
/// cannot be read, a required host command cannot be executed, or a status
/// file cannot be serialized and replaced atomically.
pub fn run() -> AppResult<()> {
    let config = Config::from_environment()?;
    let snapshot = collect(&config)?;
    write_json_atomic(&config.status_path(), &snapshot, 0o644)
}

fn collect(config: &Config) -> AppResult<StatusSnapshot> {
    let mut imports = import_units(&config.state_dir)?;
    let external_service = unit_status(
        "photo-external-sync.service",
        active_job_for_kind(&config.state_dir, "external-sync"),
    )?;
    let internxt_service = unit_status(
        "photo-internxt-backup.service",
        active_job_for_kind(&config.state_dir, "internxt-backup"),
    )?;
    let internxt_sidecar_service = unit_status(
        "photo-internxt-sidecar-backup.service",
        active_job_for_kind(&config.state_dir, "internxt-sidecars"),
    )?;
    let reconcile_service = unit_status(
        "photo-workflow-reconcile.service",
        active_job_for_kind(&config.state_dir, "reconciliation"),
    )?;

    let local_inventory = raw_directory_inventory(&config.local_root.join("raw"))?;
    let cameras = camera_inventory(config, &config.local_root.join("raw"))?;
    let (device_present, external_mount, mut external_inventory) = external_inventory(config)?;
    repair_incomplete_external_inventory(
        config,
        &local_inventory,
        external_mount.as_deref(),
        external_inventory.as_mut(),
    )?;

    let remote_inventory = internxt_inventory(config)?;
    let reconciliation = reconciliation_status(&config.reconcile_status);

    let (external_raw_sync, external_sidecars_sync) = classify_external(
        &external_service,
        device_present,
        external_mount.is_some(),
        &local_inventory,
        external_inventory.as_ref(),
    );
    let internxt_raw_sync =
        classify_internxt_raw(&internxt_service, &local_inventory.raw, &remote_inventory);
    let internxt_sidecars_sync = classify_internxt_sidecars(
        &internxt_sidecar_service,
        &local_inventory.sidecars,
        &remote_inventory,
    );

    let import_active = imports.iter().any(UnitStatus::active);
    let latest_import = imports
        .iter()
        .max_by_key(|unit| unit.last_run_epoch)
        .cloned()
        .unwrap_or_else(|| UnitStatus::not_run("photo-import"));
    let import_sync = if import_active {
        SyncState::Syncing
    } else if latest_import.result == "success" {
        SyncState::Current
    } else {
        SyncState::NotVerified
    };

    imports.sort_by(|left, right| left.name.cmp(&right.name));
    let pipeline_active = import_active
        || external_service.active()
        || internxt_service.active()
        || internxt_sidecar_service.active()
        || reconcile_service.active();
    let ssd_sync = if external_mount.is_some() {
        SyncState::Mounted
    } else if device_present {
        SyncState::Available
    } else {
        SyncState::Unavailable
    };

    Ok(StatusSnapshot {
        generated_at: iso_now()?,
        pipeline: PipelineStatus {
            active: pipeline_active,
            import_active,
            imports,
            external: external_service.clone(),
            internxt: internxt_service.clone(),
            internxt_sidecars: internxt_sidecar_service.clone(),
            reconciliation_service: reconcile_service,
            flows: PipelineFlows {
                sd_to_local_raw: FlowStatus {
                    unit: latest_import,
                    sync: import_sync,
                },
                local_to_ssd_raw: FlowStatus {
                    unit: external_service.clone(),
                    sync: external_raw_sync,
                },
                ssd_to_local_sidecars: FlowStatus {
                    unit: external_service,
                    sync: external_sidecars_sync,
                },
                local_to_internxt_raw: FlowStatus {
                    unit: internxt_service,
                    sync: internxt_raw_sync,
                },
                local_to_internxt_sidecars: FlowStatus {
                    unit: internxt_sidecar_service,
                    sync: internxt_sidecars_sync,
                },
            },
        },
        locations: Locations {
            local: LocalLocation {
                inventory: local_inventory,
                path: config.local_root.clone(),
                sync: SyncState::Source,
            },
            ssd: SsdLocation {
                device: config.external_device.clone(),
                present: device_present,
                mounted: external_mount.is_some(),
                mount_point: external_mount,
                sync: ssd_sync,
                inventory: external_inventory,
            },
            internxt: InternxtLocation {
                inventory: remote_inventory,
                sync: internxt_raw_sync,
            },
        },
        cameras,
        reconciliation,
    })
}

fn required_path(name: &str, allow_empty: bool) -> AppResult<PathBuf> {
    let value = env::var(name).map_err(|_| invalid_data(format!("{name} must be set")))?;
    if !allow_empty && value.is_empty() {
        return Err(invalid_data(format!("{name} must not be empty")).into());
    }
    Ok(PathBuf::from(value))
}

fn optional_path(name: &str, default: &str) -> AppResult<PathBuf> {
    match env::var(name) {
        Ok(value) if value.is_empty() => {
            Err(invalid_data(format!("{name} must not be empty")).into())
        }
        Ok(value) => Ok(PathBuf::from(value)),
        Err(env::VarError::NotPresent) => Ok(PathBuf::from(default)),
        Err(error) => Err(error.into()),
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
