use super::*;
use crate::camera::{camera_error, safe_raw_manifest_path};
use crate::internxt::{casefold_pattern, parse_rclone_size};
use crate::inventory::raw_directory_inventory;
use crate::status::{
    classify_external, classify_internxt_raw, classify_internxt_sidecars, reconciliation_status,
};
use crate::support::{read_json, write_json_atomic};
use crate::systemd::active_job_for_kind;
use serde_json::Value;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "electricpeak-{name}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn successful_unit(last_run_epoch: u64) -> UnitStatus {
    UnitStatus {
        name: "example.service".to_owned(),
        active_state: "inactive".to_owned(),
        result: "success".to_owned(),
        main_code: Some("exited".to_owned()),
        main_status: Some("0".to_owned()),
        started_at: Some(String::new()),
        finished_at: Some(String::new()),
        last_run_at: Some(String::new()),
        last_run_epoch,
        job: Value::Null,
    }
}

fn inventory(files: u64, bytes: u64, newest: u64) -> Inventory {
    Inventory {
        file_count: files,
        bytes,
        newest_mtime_epoch: newest,
        path_verification: None,
    }
}

fn remote(raw: &Inventory, sidecars: &Inventory) -> RemoteInventory {
    RemoteInventory {
        checked_at: Some("2026-08-26T00:00:00+00:00".to_owned()),
        layout: Some("raw-sidecars".to_owned()),
        scope: Some(INTERNXT_SCOPE.to_owned()),
        raw: Some(BasicInventory {
            file_count: raw.file_count,
            bytes: raw.bytes,
        }),
        sidecars: Some(BasicInventory {
            file_count: sidecars.file_count,
            bytes: sidecars.bytes,
        }),
        error: None,
    }
}

#[test]
fn manifest_paths_are_confined_to_raw() {
    assert_eq!(
        safe_raw_manifest_path("raw/2026/photo.cr3"),
        Some(PathBuf::from("raw/2026/photo.cr3"))
    );
    assert_eq!(safe_raw_manifest_path("raw/../../etc/shadow"), None);
    assert_eq!(safe_raw_manifest_path("edited/2026/photo.jpg"), None);
    assert_eq!(safe_raw_manifest_path("/raw/2026/photo.cr3"), None);
    assert_eq!(safe_raw_manifest_path("raw/2026/notes.txt"), None);
}

#[test]
fn inventory_handles_nested_and_non_line_oriented_names() {
    let directory = TestDirectory::new("inventory");
    let nested = directory.path().join("2026");
    fs::create_dir(&nested).expect("create inventory directory");
    fs::write(nested.join("photo\n001.CR3"), b"raw").expect("write RAW fixture");
    fs::write(nested.join("photo\n001.PHOTO-EDIT"), b"edit").expect("write sidecar fixture");
    fs::write(nested.join("photo\n001.XMP"), b"xm").expect("write XMP fixture");
    fs::write(nested.join("photo\n001.ACR"), b"acr").expect("write ACR fixture");
    fs::write(nested.join("notes.txt"), b"ignored").expect("write unmanaged fixture");
    fs::write(nested.join(".DS_Store"), b"ignored").expect("write ignored fixture");
    fs::write(nested.join("._photo.CR3"), b"ignored").expect("write AppleDouble fixture");

    let inventory = raw_directory_inventory(directory.path()).expect("collect inventory");
    assert_eq!(inventory.raw.file_count, 1);
    assert_eq!(inventory.raw.bytes, 3);
    assert_eq!(inventory.sidecars.file_count, 3);
    assert_eq!(inventory.sidecars.bytes, 9);
    assert_eq!(inventory.file_count, 4);
    assert_eq!(inventory.bytes, 12);
}

#[test]
fn active_job_uses_the_newest_running_snapshot_for_kind() {
    let directory = TestDirectory::new("active-job");
    let jobs = directory.path().join("jobs");
    fs::create_dir(&jobs).expect("create jobs directory");
    fs::write(
        jobs.join("older.json"),
        br#"{"kind":"external-sync","state":"running","updatedAt":"2026-08-26T01:00:00+00:00"}"#,
    )
    .expect("write older job");
    fs::write(
        jobs.join("newer.json"),
        br#"{"kind":"external-sync","state":"running","updatedAt":"2026-08-26T02:00:00+00:00"}"#,
    )
    .expect("write newer job");
    fs::write(
        jobs.join("finished.json"),
        br#"{"kind":"external-sync","state":"success","updatedAt":"2026-08-26T03:00:00+00:00"}"#,
    )
    .expect("write finished job");

    let job = active_job_for_kind(directory.path(), "external-sync");
    assert_eq!(job["updatedAt"], "2026-08-26T02:00:00+00:00");
    assert_eq!(
        active_job_for_kind(directory.path(), "internxt-backup"),
        Value::Null
    );
}

#[test]
fn rclone_filters_are_case_insensitive() {
    assert_eq!(casefold_pattern("cr3"), "*.[cC][rR]3");
    assert_eq!(
        casefold_pattern("photo-edit"),
        "*.[pP][hH][oO][tT][oO]-[eE][dD][iI][tT]"
    );
}

#[test]
fn atomic_json_write_replaces_the_complete_document() {
    let directory = TestDirectory::new("atomic-json");
    let status_path = directory.path().join("status.json");
    fs::write(&status_path, b"old").expect("write previous status");

    write_json_atomic(
        &status_path,
        &serde_json::json!({ "state": "current", "count": 2 }),
        0o644,
    )
    .expect("replace status");

    let value: Value = read_json(&status_path).expect("read replaced status");
    assert_eq!(value["state"], "current");
    assert_eq!(value["count"], 2);
    assert_eq!(
        fs::metadata(&status_path)
            .expect("read status metadata")
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    assert_eq!(
        fs::read_dir(directory.path())
            .expect("list test directory")
            .count(),
        1
    );
}

#[test]
fn parses_rclone_size_without_accepting_partial_objects() {
    assert_eq!(
        parse_rclone_size(br#"{"count":3,"bytes":42,"trashed":0}"#),
        Some(BasicInventory {
            file_count: 3,
            bytes: 42
        })
    );
    assert_eq!(parse_rclone_size(br#"{"count":3}"#), None);
    assert_eq!(parse_rclone_size(b"not json"), None);
}

#[test]
fn external_classification_preserves_priority() {
    let local = RawDirectoryInventory::new(inventory(2, 20, 100), inventory(1, 5, 100));
    let external = local.clone();
    let mut active = successful_unit(200);
    active.active_state = "active".to_owned();
    assert_eq!(
        classify_external(&active, true, true, &local, Some(&external)),
        (SyncState::Syncing, SyncState::Syncing)
    );
    assert_eq!(
        classify_external(&successful_unit(200), false, false, &local, None),
        (SyncState::Unavailable, SyncState::Unavailable)
    );
    assert_eq!(
        classify_external(&successful_unit(200), true, false, &local, None),
        (SyncState::MountPending, SyncState::MountPending)
    );
    assert_eq!(
        classify_external(
            &UnitStatus::not_run("external"),
            true,
            true,
            &local,
            Some(&external)
        ),
        (SyncState::NotVerified, SyncState::NotVerified)
    );
}

#[test]
fn external_classification_detects_behind_and_mismatch() {
    let local = RawDirectoryInventory::new(inventory(2, 20, 300), inventory(1, 5, 100));
    let external = RawDirectoryInventory::new(inventory(2, 20, 100), inventory(2, 10, 300));
    assert_eq!(
        classify_external(&successful_unit(200), true, true, &local, Some(&external)),
        (SyncState::Behind, SyncState::Behind)
    );

    let local = RawDirectoryInventory::new(inventory(2, 20, 100), inventory(1, 5, 100));
    let external = RawDirectoryInventory::new(inventory(1, 10, 100), inventory(0, 0, 100));
    assert_eq!(
        classify_external(&successful_unit(200), true, true, &local, Some(&external)),
        (SyncState::Mismatch, SyncState::Mismatch)
    );
}

#[test]
fn internxt_classification_handles_errors_and_inventory() {
    let raw = inventory(2, 20, 100);
    let sidecars = inventory(1, 5, 100);
    let service = successful_unit(200);
    let current = remote(&raw, &sidecars);
    assert_eq!(
        classify_internxt_raw(&service, &raw, &current),
        SyncState::Current
    );
    assert_eq!(
        classify_internxt_sidecars(&service, &sidecars, &current),
        SyncState::Current
    );

    let failed = RemoteInventory::unavailable(None, "offline");
    assert_eq!(
        classify_internxt_raw(&service, &raw, &failed),
        SyncState::Unknown
    );
    assert_eq!(
        classify_internxt_raw(&service, &inventory(2, 20, 300), &current),
        SyncState::Behind
    );
}

#[test]
fn serialized_snapshot_keeps_the_dashboard_contract() {
    let unit = successful_unit(200);
    let local_inventory = RawDirectoryInventory::new(inventory(2, 20, 100), inventory(1, 5, 100));
    let remote_inventory = remote(&local_inventory.raw, &local_inventory.sidecars);
    let snapshot = StatusSnapshot {
        generated_at: "2026-08-26T00:00:00+00:00".to_owned(),
        pipeline: PipelineStatus {
            active: false,
            import_active: false,
            imports: Vec::new(),
            external: unit.clone(),
            internxt: unit.clone(),
            internxt_sidecars: unit.clone(),
            reconciliation_service: unit.clone(),
            flows: PipelineFlows {
                sd_to_local_raw: FlowStatus {
                    unit: UnitStatus::not_run("photo-import"),
                    sync: SyncState::NotVerified,
                },
                local_to_ssd_raw: FlowStatus {
                    unit: unit.clone(),
                    sync: SyncState::Current,
                },
                ssd_to_local_sidecars: FlowStatus {
                    unit: unit.clone(),
                    sync: SyncState::Current,
                },
                local_to_internxt_raw: FlowStatus {
                    unit: unit.clone(),
                    sync: SyncState::Current,
                },
                local_to_internxt_sidecars: FlowStatus {
                    unit,
                    sync: SyncState::Current,
                },
            },
        },
        locations: Locations {
            local: LocalLocation {
                inventory: local_inventory,
                path: PathBuf::from("/photos"),
                sync: SyncState::Source,
            },
            ssd: SsdLocation {
                device: PathBuf::from("/dev/example"),
                present: false,
                mounted: false,
                mount_point: None,
                sync: SyncState::Unavailable,
                inventory: None,
            },
            internxt: InternxtLocation {
                inventory: remote_inventory,
                sync: SyncState::Current,
            },
        },
        cameras: camera_error("2026-08-26T00:00:00+00:00".to_owned(), None, "No import"),
        reconciliation: serde_json::json!({ "state": "idle" }),
    };

    let value = serde_json::to_value(snapshot).expect("serialize snapshot");
    assert_eq!(value["generatedAt"], "2026-08-26T00:00:00+00:00");
    assert_eq!(value["pipeline"]["importActive"], false);
    assert_eq!(
        value["pipeline"]["flows"]["sdToLocalRaw"]["sync"],
        "not-verified"
    );
    assert_eq!(value["locations"]["local"]["raw"]["fileCount"], 2);
    assert_eq!(value["locations"]["ssd"]["mountPoint"], Value::Null);
    assert_eq!(value["locations"]["internxt"]["raw"]["bytes"], 20);
    assert_eq!(value["cameras"]["checkedAt"], "2026-08-26T00:00:00+00:00");
    assert_eq!(value["reconciliation"]["state"], "idle");
}

#[test]
fn reconciliation_requires_a_string_state() {
    let missing = reconciliation_status(Path::new("/definitely/not/present"));
    assert_eq!(missing["state"], "idle");
}
