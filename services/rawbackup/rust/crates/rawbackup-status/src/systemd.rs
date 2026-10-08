use crate::support::{command_output, read_json, successful_stdout};
use crate::{AppResult, UnitStatus};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

pub(crate) fn active_job_for_kind(state_dir: &Path, kind: &str) -> Value {
    let Ok(entries) = fs::read_dir(state_dir.join("jobs")) else {
        return Value::Null;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
        .filter_map(|entry| read_json::<Value>(&entry.path()))
        .filter(|job| {
            job.get("kind").and_then(Value::as_str) == Some(kind)
                && job.get("state").and_then(Value::as_str) == Some("running")
                && job.get("updatedAt").is_some_and(Value::is_string)
        })
        .max_by(|left, right| {
            left.get("updatedAt")
                .and_then(Value::as_str)
                .cmp(&right.get("updatedAt").and_then(Value::as_str))
        })
        .unwrap_or(Value::Null)
}

pub(crate) fn import_units(state_dir: &Path) -> AppResult<Vec<UnitStatus>> {
    let output = command_output(
        "systemctl",
        [
            "list-units",
            "--all",
            "--type=service",
            "--full",
            "--no-legend",
            "photo-import@*.service",
        ],
    )?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let job = active_job_for_kind(state_dir, "sd-import");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .find(|field| field.ends_with(".service"))
        })
        .map(|unit| unit_status(unit, job.clone()))
        .collect()
}

pub(crate) fn unit_status(unit: &str, job: Value) -> AppResult<UnitStatus> {
    let active_state = property(unit, "ActiveState")?;
    let mut result = property(unit, "Result")?;
    let main_code = property(unit, "ExecMainCode")?;
    let main_status = property(unit, "ExecMainStatus")?;
    if main_code == "exited" && main_status.parse::<i32>().is_ok_and(|status| status != 0) {
        "failed".clone_into(&mut result);
    }
    let start_epoch = timestamp_to_epoch(&property(unit, "ExecMainStartTimestamp")?)?;
    let exit_epoch = timestamp_to_epoch(&property(unit, "ExecMainExitTimestamp")?)?;
    let last_run_epoch = if exit_epoch > 0 {
        exit_epoch
    } else {
        start_epoch
    };
    Ok(UnitStatus {
        name: unit.to_owned(),
        active_state: nonempty_or(active_state, "unknown"),
        result: nonempty_or(result, "unknown"),
        main_code: Some(nonempty_or(main_code, "unknown")),
        main_status: Some(nonempty_or(main_status, "unknown")),
        started_at: Some(epoch_to_iso(start_epoch)?),
        finished_at: Some(epoch_to_iso(exit_epoch)?),
        last_run_at: Some(epoch_to_iso(last_run_epoch)?),
        last_run_epoch,
        job,
    })
}

pub(crate) fn property(unit: &str, name: &str) -> AppResult<String> {
    let output = command_output(
        "systemctl",
        [
            OsString::from("show"),
            OsString::from(unit),
            OsString::from(format!("--property={name}")),
            OsString::from("--value"),
            OsString::from("--no-pager"),
        ],
    )?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned())
    } else {
        Ok(String::new())
    }
}

pub(crate) fn nonempty_or(value: String, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

pub(crate) fn timestamp_to_epoch(timestamp: &str) -> AppResult<u64> {
    if timestamp.is_empty() || timestamp == "n/a" {
        return Ok(0);
    }
    let output = command_output(
        "date",
        [
            OsString::from(format!("--date={timestamp}")),
            OsString::from("+%s"),
        ],
    )?;
    if !output.status.success() {
        return Ok(0);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0))
}

pub(crate) fn epoch_to_iso(epoch: u64) -> AppResult<String> {
    if epoch == 0 {
        return Ok(String::new());
    }
    successful_stdout(
        "date",
        [
            OsString::from("--utc"),
            OsString::from(format!("--date=@{epoch}")),
            OsString::from("--iso-8601=seconds"),
        ],
    )
}

pub(crate) fn iso_now() -> AppResult<String> {
    successful_stdout("date", ["--utc", "--iso-8601=seconds"])
}
