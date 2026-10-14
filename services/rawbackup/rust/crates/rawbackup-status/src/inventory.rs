use crate::support::command_output;
use crate::{
    AppResult, Config, Inventory, InventoryKind, PathVerification, RAW_EXTENSIONS,
    RawDirectoryInventory, SIDECAR_EXTENSIONS,
};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub(crate) fn is_ignored(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name == ".DS_Store" || name.starts_with("._")
}

pub(crate) fn has_extension(name: &OsStr, extensions: &[&str]) -> bool {
    !is_ignored(name)
        && Path::new(name)
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
            .is_some_and(|extension| extensions.contains(&extension.as_str()))
}

pub(crate) fn is_raw(name: &OsStr) -> bool {
    has_extension(name, RAW_EXTENSIONS)
}

pub(crate) fn is_sidecar(name: &OsStr) -> bool {
    has_extension(name, SIDECAR_EXTENSIONS)
}

pub(crate) fn matches_inventory_kind(name: &OsStr, kind: InventoryKind) -> bool {
    match kind {
        InventoryKind::Raw => is_raw(name),
        InventoryKind::Sidecar => is_sidecar(name),
    }
}

pub(crate) fn raw_directory_inventory(root: &Path) -> AppResult<RawDirectoryInventory> {
    Ok(RawDirectoryInventory::new(
        file_inventory(root, InventoryKind::Raw)?,
        file_inventory(root, InventoryKind::Sidecar)?,
    ))
}

pub(crate) fn file_inventory(root: &Path, kind: InventoryKind) -> AppResult<Inventory> {
    let mut inventory = Inventory::default();
    if !root.is_dir() {
        return Ok(inventory);
    }
    visit_files(root, root, &mut |_, metadata| {
        let name = metadata.path.file_name().unwrap_or_else(|| OsStr::new(""));
        if !matches_inventory_kind(name, kind) {
            return Ok(());
        }
        inventory.file_count += 1;
        inventory.bytes += metadata.metadata.len();
        let modified_duration = metadata
            .metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let modified = modified_duration
            .as_secs()
            .saturating_add(u64::from(modified_duration.subsec_nanos() >= 500_000_000));
        inventory.newest_mtime_epoch = inventory.newest_mtime_epoch.max(modified);
        Ok(())
    })?;
    Ok(inventory)
}

struct VisitedFile {
    path: PathBuf,
    metadata: fs::Metadata,
}

fn visit_files(
    root: &Path,
    directory: &Path,
    visitor: &mut impl FnMut(&Path, &VisitedFile) -> AppResult<()>,
) -> AppResult<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            visit_files(root, &entry.path(), visitor)?;
        } else if file_type.is_file() {
            let file = VisitedFile {
                path: entry.path(),
                metadata: entry.metadata()?,
            };
            visitor(root, &file)?;
        }
    }
    Ok(())
}

pub(crate) fn path_presence_check(source: &Path, destination: &Path) -> AppResult<(u64, u64)> {
    let mut checked = 0;
    let mut present = 0;
    if !source.is_dir() {
        return Ok((checked, present));
    }
    visit_files(source, source, &mut |root, file| {
        let name = file.path.file_name().unwrap_or_else(|| OsStr::new(""));
        if !is_raw(name) {
            return Ok(());
        }
        checked += 1;
        let relative = file.path.strip_prefix(root)?;
        if destination.join(relative).is_file() {
            present += 1;
        }
        Ok(())
    })?;
    Ok((checked, present))
}

pub(crate) fn external_inventory(
    config: &Config,
) -> AppResult<(bool, Option<PathBuf>, Option<RawDirectoryInventory>)> {
    if !config.external_device.exists() {
        return Ok((false, None, None));
    }
    let resolved_device = fs::canonicalize(&config.external_device)?;
    let output = command_output(
        "findmnt",
        [
            OsString::from("--noheadings"),
            OsString::from("--raw"),
            OsString::from("--output"),
            OsString::from("TARGET"),
            OsString::from("--source"),
            resolved_device.into_os_string(),
        ],
    )?;
    if !output.status.success() {
        return Ok((true, None, None));
    }
    let mount = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if mount.is_empty() {
        return Ok((true, None, None));
    }
    let mount = PathBuf::from(mount);
    let external_root = mount.join(&config.external_subdirectory);
    let inventory = if external_root.is_dir() {
        Some(raw_directory_inventory(
            &external_root.join(&config.external_raw_subdirectory),
        )?)
    } else {
        Some(RawDirectoryInventory::default())
    };
    Ok((true, Some(mount), inventory))
}

pub(crate) fn repair_incomplete_external_inventory(
    config: &Config,
    local: &RawDirectoryInventory,
    external_mount: Option<&Path>,
    external: Option<&mut RawDirectoryInventory>,
) -> AppResult<()> {
    let (Some(mount), Some(external)) = (external_mount, external) else {
        return Ok(());
    };
    let enumerated = external.raw.file_count;
    if enumerated >= local.raw.file_count {
        return Ok(());
    }
    let external_raw = mount
        .join(&config.external_subdirectory)
        .join(&config.external_raw_subdirectory);
    let (checked, present) = path_presence_check(&config.local_root.join("raw"), &external_raw)?;
    if checked == present {
        external.raw = local.raw.clone();
        external.raw.path_verification = Some(PathVerification {
            checked,
            present,
            missing: checked - present,
            enumeration_incomplete: true,
            enumerated_file_count: enumerated,
        });
        external.refresh_totals();
    }
    Ok(())
}
