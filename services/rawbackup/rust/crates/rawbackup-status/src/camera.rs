use crate::inventory::is_raw;
use crate::support::{command_output, read_json, write_json_atomic};
use crate::systemd::iso_now;
use crate::{AppResult, CameraAudit, CameraStatus, Config, ExifRecord, ImportManifest};
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

pub(crate) fn camera_inventory(config: &Config, raw_root: &Path) -> AppResult<CameraAudit> {
    let checked_at = iso_now()?;
    let Some(manifest) = read_json::<ImportManifest>(&config.sd_import_manifest) else {
        return Ok(camera_error(
            checked_at,
            None,
            "No successful SD import has produced a RAW manifest yet.",
        ));
    };
    if manifest.job_id.is_empty() {
        return Ok(camera_error(
            checked_at,
            None,
            "No successful SD import has produced a RAW manifest yet.",
        ));
    }

    let camera_audit_path = config.camera_audit_path();
    if let Some(cached) = read_json::<CameraAudit>(&camera_audit_path) {
        if cached.import_job_id.as_deref() == Some(manifest.job_id.as_str()) {
            return Ok(cached);
        }
    }

    let library_root = raw_root.parent().unwrap_or(raw_root);
    let raw_files: Vec<PathBuf> = manifest
        .raw_files
        .iter()
        .filter_map(Value::as_str)
        .filter_map(safe_raw_manifest_path)
        .map(|relative| library_root.join(relative))
        .filter(|path| path.is_file())
        .collect();
    if raw_files.is_empty() {
        return Ok(camera_error(
            checked_at,
            Some(manifest.job_id),
            "The latest SD import RAW files are no longer available locally.",
        ));
    }

    let mut arguments = vec![
        OsString::from("300"),
        OsString::from("exiftool"),
        OsString::from("-json"),
        OsString::from("-DateTimeOriginal"),
        OsString::from("-CreateDate"),
        OsString::from("-Model"),
    ];
    arguments.extend(raw_files.iter().map(|path| path.as_os_str().to_owned()));
    let output = command_output("timeout", arguments)?;
    let warning = if output.status.code() == Some(124) {
        Some("Camera inspection timed out before all SD-imported RAW files could be read.")
    } else if output.status.success() {
        None
    } else {
        Some("Some SD-imported RAW files could not be inspected.")
    };

    let Ok(records) = serde_json::from_slice::<Vec<ExifRecord>>(&output.stdout) else {
        return Ok(camera_error(
            checked_at,
            Some(manifest.job_id),
            "Could not inspect the latest SD import RAW metadata.",
        ));
    };
    let mut latest_by_camera = BTreeMap::<String, String>::new();
    for record in records {
        let Some(captured_at) = record.date_time_original.or(record.create_date) else {
            continue;
        };
        let camera = record.model.unwrap_or_else(|| "Unknown camera".to_owned());
        latest_by_camera
            .entry(camera)
            .and_modify(|latest| {
                if captured_at > *latest {
                    latest.clone_from(&captured_at);
                }
            })
            .or_insert(captured_at);
    }
    let audit = CameraAudit {
        checked_at,
        import_job_id: Some(manifest.job_id),
        import_completed_at: manifest.completed_at.filter(|value| !value.is_empty()),
        source_device: manifest.source_device.filter(|value| !value.is_empty()),
        files_inspected: Some(raw_files.len()),
        cameras: latest_by_camera
            .into_iter()
            .map(|(camera, latest_capture_at)| CameraStatus {
                camera,
                latest_capture_at,
            })
            .collect(),
        error: warning.map(str::to_owned),
    };
    write_json_atomic(&camera_audit_path, &audit, 0o600)?;
    Ok(audit)
}

pub(crate) fn camera_error(
    checked_at: String,
    import_job_id: Option<String>,
    message: &str,
) -> CameraAudit {
    CameraAudit {
        checked_at,
        import_job_id,
        import_completed_at: None,
        source_device: None,
        files_inspected: None,
        cameras: Vec::new(),
        error: Some(message.to_owned()),
    }
}

pub(crate) fn safe_raw_manifest_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    let mut components = path.components();
    if components.next() != Some(Component::Normal(OsStr::new("raw")))
        || !components
            .clone()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    path.file_name().filter(|name| is_raw(name))?;
    Some(path.to_owned())
}
