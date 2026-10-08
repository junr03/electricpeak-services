use crate::support::read_json;
use crate::{Inventory, RawDirectoryInventory, RemoteInventory, SyncState, UnitStatus};
use serde_json::Value;
use std::path::Path;

pub(crate) fn reconciliation_status(path: &Path) -> Value {
    read_json::<Value>(path)
        .filter(|status| status.get("state").is_some_and(Value::is_string))
        .unwrap_or_else(|| {
            serde_json::json!({
                "state": "idle",
                "result": null,
                "message": "No reconciliation has been requested.",
                "startedAt": null,
                "finishedAt": null,
                "reportPath": null,
                "inventories": null
            })
        })
}

pub(crate) fn classify_external(
    service: &UnitStatus,
    device_present: bool,
    mounted: bool,
    local: &RawDirectoryInventory,
    external: Option<&RawDirectoryInventory>,
) -> (SyncState, SyncState) {
    if service.active() {
        return (SyncState::Syncing, SyncState::Syncing);
    }
    if !device_present {
        return (SyncState::Unavailable, SyncState::Unavailable);
    }
    if !mounted {
        return (SyncState::MountPending, SyncState::MountPending);
    }
    if service.never_ran_successfully() {
        return (SyncState::NotVerified, SyncState::NotVerified);
    }
    let Some(external) = external else {
        return (SyncState::Unknown, SyncState::Unknown);
    };
    let raw = if external
        .raw
        .path_verification
        .as_ref()
        .is_some_and(|check| check.enumeration_incomplete)
    {
        SyncState::InventoryIncomplete
    } else if local.raw.newest_mtime_epoch > service.last_run_epoch {
        SyncState::Behind
    } else if external.raw.file_count != local.raw.file_count
        || external.raw.bytes != local.raw.bytes
    {
        SyncState::Mismatch
    } else {
        SyncState::Current
    };
    let sidecars = if external.sidecars.newest_mtime_epoch > service.last_run_epoch
        || external.sidecars.file_count > local.sidecars.file_count
        || external.sidecars.bytes > local.sidecars.bytes
    {
        SyncState::Behind
    } else if external.sidecars.file_count != local.sidecars.file_count
        || external.sidecars.bytes != local.sidecars.bytes
    {
        SyncState::Mismatch
    } else {
        SyncState::Current
    };
    (raw, sidecars)
}

pub(crate) fn classify_internxt_raw(
    service: &UnitStatus,
    local: &Inventory,
    remote: &RemoteInventory,
) -> SyncState {
    if service.active() {
        SyncState::Syncing
    } else if service.never_ran_successfully() {
        SyncState::NotVerified
    } else if remote.error.is_some() {
        SyncState::Unknown
    } else if local.newest_mtime_epoch > service.last_run_epoch {
        SyncState::Behind
    } else if remote.raw.as_ref().is_none_or(|inventory| {
        inventory.file_count != local.file_count || inventory.bytes != local.bytes
    }) {
        SyncState::Mismatch
    } else {
        SyncState::Current
    }
}

pub(crate) fn classify_internxt_sidecars(
    service: &UnitStatus,
    local: &Inventory,
    remote: &RemoteInventory,
) -> SyncState {
    if service.active() {
        SyncState::Syncing
    } else if service.never_ran_successfully() {
        SyncState::NotVerified
    } else if remote.error.is_some() {
        SyncState::Unknown
    } else if remote.sidecars.as_ref().is_none_or(|inventory| {
        inventory.file_count != local.file_count || inventory.bytes != local.bytes
    }) {
        SyncState::Mismatch
    } else {
        SyncState::Current
    }
}
