use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use crate::INTERNXT_SCOPE;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Inventory {
    pub(crate) file_count: u64,
    pub(crate) bytes: u64,
    pub(crate) newest_mtime_epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) path_verification: Option<PathVerification>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PathVerification {
    pub(crate) checked: u64,
    pub(crate) present: u64,
    pub(crate) missing: u64,
    pub(crate) enumeration_incomplete: bool,
    pub(crate) enumerated_file_count: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawDirectoryInventory {
    pub(crate) raw: Inventory,
    pub(crate) sidecars: Inventory,
    pub(crate) file_count: u64,
    pub(crate) bytes: u64,
    pub(crate) newest_mtime_epoch: u64,
}

impl RawDirectoryInventory {
    pub(crate) fn new(raw: Inventory, sidecars: Inventory) -> Self {
        Self {
            file_count: raw.file_count + sidecars.file_count,
            bytes: raw.bytes + sidecars.bytes,
            newest_mtime_epoch: raw.newest_mtime_epoch.max(sidecars.newest_mtime_epoch),
            raw,
            sidecars,
        }
    }

    pub(crate) fn refresh_totals(&mut self) {
        self.file_count = self.raw.file_count + self.sidecars.file_count;
        self.bytes = self.raw.bytes + self.sidecars.bytes;
        self.newest_mtime_epoch = self
            .raw
            .newest_mtime_epoch
            .max(self.sidecars.newest_mtime_epoch);
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BasicInventory {
    pub(crate) file_count: u64,
    pub(crate) bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CameraAudit {
    pub(crate) checked_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) import_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) import_completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) files_inspected: Option<usize>,
    pub(crate) cameras: Vec<CameraStatus>,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CameraStatus {
    pub(crate) camera: String,
    pub(crate) latest_capture_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportManifest {
    pub(crate) job_id: String,
    #[serde(default)]
    pub(crate) completed_at: Option<String>,
    #[serde(default)]
    pub(crate) source_device: Option<String>,
    pub(crate) raw_files: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ExifRecord {
    #[serde(default)]
    pub(crate) date_time_original: Option<String>,
    #[serde(default)]
    pub(crate) create_date: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnitStatus {
    pub(crate) name: String,
    pub(crate) active_state: String,
    pub(crate) result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) main_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) main_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) finished_at: Option<String>,
    pub(crate) last_run_at: Option<String>,
    pub(crate) last_run_epoch: u64,
    pub(crate) job: Value,
}

impl UnitStatus {
    pub(crate) fn active(&self) -> bool {
        matches!(self.active_state.as_str(), "active" | "activating")
    }

    pub(crate) fn never_ran_successfully(&self) -> bool {
        self.result != "success" || self.last_run_epoch == 0
    }

    pub(crate) fn not_run(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            active_state: "inactive".to_owned(),
            result: "not run".to_owned(),
            main_code: None,
            main_status: None,
            started_at: None,
            finished_at: None,
            last_run_at: None,
            last_run_epoch: 0,
            job: Value::Null,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SyncState {
    Available,
    Behind,
    Current,
    InventoryIncomplete,
    Mismatch,
    Mounted,
    MountPending,
    NotVerified,
    Source,
    Syncing,
    Unavailable,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteInventory {
    pub(crate) checked_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) scope: Option<String>,
    pub(crate) raw: Option<BasicInventory>,
    pub(crate) sidecars: Option<BasicInventory>,
    pub(crate) error: Option<String>,
}

impl RemoteInventory {
    pub(crate) fn usable_cache(&self) -> bool {
        self.layout.as_deref() == Some("raw-sidecars")
            && self.scope.as_deref() == Some(INTERNXT_SCOPE)
            && self.raw.is_some()
            && self.sidecars.is_some()
    }

    pub(crate) fn unavailable(checked_at: Option<String>, message: &str) -> Self {
        Self {
            checked_at,
            layout: Some("raw-sidecars".to_owned()),
            scope: Some(INTERNXT_SCOPE.to_owned()),
            raw: None,
            sidecars: None,
            error: Some(message.to_owned()),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FlowStatus {
    #[serde(flatten)]
    pub(crate) unit: UnitStatus,
    pub(crate) sync: SyncState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PipelineFlows {
    pub(crate) sd_to_local_raw: FlowStatus,
    pub(crate) local_to_ssd_raw: FlowStatus,
    pub(crate) ssd_to_local_sidecars: FlowStatus,
    pub(crate) local_to_internxt_raw: FlowStatus,
    pub(crate) local_to_internxt_sidecars: FlowStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PipelineStatus {
    pub(crate) active: bool,
    pub(crate) import_active: bool,
    pub(crate) imports: Vec<UnitStatus>,
    pub(crate) external: UnitStatus,
    pub(crate) internxt: UnitStatus,
    pub(crate) internxt_sidecars: UnitStatus,
    pub(crate) reconciliation_service: UnitStatus,
    pub(crate) flows: PipelineFlows,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalLocation {
    #[serde(flatten)]
    pub(crate) inventory: RawDirectoryInventory,
    pub(crate) path: PathBuf,
    pub(crate) sync: SyncState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SsdLocation {
    pub(crate) device: PathBuf,
    pub(crate) present: bool,
    pub(crate) mounted: bool,
    pub(crate) mount_point: Option<PathBuf>,
    pub(crate) sync: SyncState,
    pub(crate) inventory: Option<RawDirectoryInventory>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InternxtLocation {
    #[serde(flatten)]
    pub(crate) inventory: RemoteInventory,
    pub(crate) sync: SyncState,
}

#[derive(Debug, Serialize)]
pub(crate) struct Locations {
    pub(crate) local: LocalLocation,
    pub(crate) ssd: SsdLocation,
    pub(crate) internxt: InternxtLocation,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusSnapshot {
    pub(crate) generated_at: String,
    pub(crate) pipeline: PipelineStatus,
    pub(crate) locations: Locations,
    pub(crate) cameras: CameraAudit,
    pub(crate) reconciliation: Value,
}

#[derive(Clone, Copy)]
pub(crate) enum InventoryKind {
    Raw,
    Sidecar,
}
