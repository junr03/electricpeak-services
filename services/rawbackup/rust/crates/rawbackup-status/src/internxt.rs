use crate::support::{command_output, read_json, write_json_atomic};
use crate::systemd::iso_now;
use crate::{
    AUDIT_MAX_AGE_SECONDS, AppResult, BasicInventory, Config, INTERNXT_RAW_PATH, INTERNXT_SCOPE,
    RAW_EXTENSIONS, RemoteInventory, SIDECAR_EXTENSIONS,
};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

pub(crate) fn internxt_inventory(config: &Config) -> AppResult<RemoteInventory> {
    let audit_path = config.internxt_audit_path();
    let cached = read_json::<RemoteInventory>(&audit_path);
    let fresh = cached.as_ref().is_some_and(RemoteInventory::usable_cache)
        && file_age_seconds(&audit_path).is_some_and(|age| age < AUDIT_MAX_AGE_SECONDS);
    if fresh {
        return Ok(cached.expect("a fresh cache must be present"));
    }

    if config.internxt_config_path.is_file() {
        if let Some(inventory) = query_internxt(config)? {
            write_json_atomic(&audit_path, &inventory, 0o644)?;
            return Ok(inventory);
        }
        if let Some(inventory) = cached.filter(RemoteInventory::usable_cache) {
            return Ok(inventory);
        }
        let unavailable =
            RemoteInventory::unavailable(Some(iso_now()?), "Could not query Internxt");
        write_json_atomic(&audit_path, &unavailable, 0o644)?;
        return Ok(unavailable);
    }

    Ok(cached.unwrap_or_else(|| RemoteInventory {
        checked_at: None,
        layout: None,
        scope: None,
        raw: None,
        sidecars: None,
        error: Some("No Internxt audit yet".to_owned()),
    }))
}

pub(crate) fn query_internxt(config: &Config) -> AppResult<Option<RemoteInventory>> {
    let Some(raw) = query_internxt_inventory(config, RAW_EXTENSIONS)? else {
        return Ok(None);
    };
    let Some(sidecars) = query_internxt_inventory(config, SIDECAR_EXTENSIONS)? else {
        return Ok(None);
    };
    Ok(Some(RemoteInventory {
        checked_at: Some(iso_now()?),
        layout: Some("raw-sidecars".to_owned()),
        scope: Some(INTERNXT_SCOPE.to_owned()),
        raw: Some(raw),
        sidecars: Some(sidecars),
        error: None,
    }))
}

pub(crate) fn query_internxt_inventory(
    config: &Config,
    extensions: &[&str],
) -> AppResult<Option<BasicInventory>> {
    let mut arguments = vec![
        OsString::from("240"),
        OsString::from("rclone"),
        OsString::from("size"),
        OsString::from(INTERNXT_RAW_PATH),
        OsString::from("--filter=- ._*"),
    ];
    arguments.extend(
        extensions
            .iter()
            .map(|extension| OsString::from(format!("--filter=+ {}", casefold_pattern(extension)))),
    );
    arguments.extend([
        OsString::from("--filter=- **"),
        OsString::from("--config"),
        config.internxt_config_path.as_os_str().to_owned(),
        OsString::from("--json"),
    ]);
    let output = command_output("timeout", arguments)?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(parse_rclone_size(&output.stdout))
}

pub(crate) fn casefold_pattern(extension: &str) -> String {
    let mut pattern = String::from("*.");
    for character in extension.chars() {
        if character.is_ascii_alphabetic() {
            pattern.push('[');
            pattern.push(character.to_ascii_lowercase());
            pattern.push(character.to_ascii_uppercase());
            pattern.push(']');
        } else {
            pattern.push(character);
        }
    }
    pattern
}

pub(crate) fn parse_rclone_size(output: &[u8]) -> Option<BasicInventory> {
    let value = serde_json::from_slice::<Value>(output).ok()?;
    Some(BasicInventory {
        file_count: value.get("count")?.as_u64()?,
        bytes: value.get("bytes")?.as_u64()?,
    })
}

pub(crate) fn file_age_seconds(path: &Path) -> Option<u64> {
    SystemTime::now()
        .duration_since(fs::metadata(path).ok()?.modified().ok()?)
        .ok()
        .map(|duration| duration.as_secs())
}
