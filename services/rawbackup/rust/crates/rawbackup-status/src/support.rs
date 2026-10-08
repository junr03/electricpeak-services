use crate::{AppResult, TEMP_FILE_SEQUENCE, invalid_data};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_reader(File::open(path).ok()?).ok()
}

pub(crate) fn write_json_atomic(path: &Path, value: &impl Serialize, mode: u32) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_data(format!("{} has no parent directory", path.display())))?;
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{}.{}.{}.{}.tmp",
        path.file_name()
            .unwrap_or_else(|| OsStr::new("status"))
            .to_string_lossy(),
        std::process::id(),
        timestamp,
        sequence
    ));
    let result = (|| -> AppResult<()> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, value)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn successful_stdout<I, S>(program: &str, arguments: I) -> AppResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = command_output(program, arguments)?;
    if !output.status.success() {
        return Err(invalid_data(format!("{program} exited with status {}", output.status)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub(crate) fn command_output<I, S>(program: &str, arguments: I) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(program).args(arguments).output()
}
