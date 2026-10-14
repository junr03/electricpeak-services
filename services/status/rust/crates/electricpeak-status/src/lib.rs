//! Collects a public-safe health snapshot of the home server: `ZeroTier`,
//! `Tailscale`, SSH, and Docker containers. A timer runs the collector on the
//! host; a static web container only serves the resulting JSON.

mod model;
mod parse;
mod probes;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use model::{Check, Container, Overall, Snapshot, Status};
use probes::{CommandError, run};

pub struct Config {
    pub output: PathBuf,
    pub ssh_address: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            output: std::env::var_os("ELECTRICPEAK_STATUS_OUTPUT").map_or_else(
                || PathBuf::from("/var/lib/electricpeak-status/status.json"),
                PathBuf::from,
            ),
            ssh_address: std::env::var("ELECTRICPEAK_STATUS_SSH_ADDRESS")
                .unwrap_or_else(|_| "127.0.0.1:22".to_owned()),
        }
    }
}

#[must_use]
pub fn collect(config: &Config) -> Snapshot {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let checks = vec![zerotier(), tailscale(), {
        let (status, detail) = probes::ssh(&config.ssh_address);
        Check::new("ssh", "SSH (port 22)", status, detail)
    }];
    Snapshot::new(now, checks, containers(&docker_state()))
}

fn tailscale() -> Check {
    let (status, detail) = match run("tailscale", &["status", "--json"]) {
        Ok(json) => parse::tailscale(&json),
        Err(error) => unavailable("tailscale", &error),
    };
    Check::new("tailscale", "Tailscale", status, detail)
}

fn zerotier() -> Check {
    let result = run("zerotier-cli", &["-j", "info"]).and_then(|info| {
        run("zerotier-cli", &["-j", "listnetworks"]).map(|networks| (info, networks))
    });
    let (status, detail) = match result {
        Ok((info, networks)) => parse::zerotier(&info, &networks),
        Err(error) => unavailable("zerotier", &error),
    };
    Check::new("zerotier", "ZeroTier", status, detail)
}

/// A missing tool is `Unknown`. A tool that fails or hangs means its daemon
/// is not answering, which is an outage.
fn unavailable(service: &str, error: &CommandError) -> (Status, String) {
    match error {
        CommandError::Missing => (
            Status::Unknown,
            format!("{service} is not installed on this host"),
        ),
        CommandError::Failed => (Status::Down, format!("{service} daemon is not responding")),
    }
}

enum DockerResult {
    Output(String),
    Error(CommandError),
}

fn docker_state() -> DockerResult {
    let ids = match run("docker", &["ps", "-a", "-q", "--no-trunc"]) {
        Ok(ids) => ids,
        Err(error) => return DockerResult::Error(error),
    };
    let ids: Vec<&str> = ids.split_whitespace().collect();
    if ids.is_empty() {
        return DockerResult::Output(String::new());
    }
    let mut args = vec!["inspect", "--format", parse::INSPECT_FORMAT];
    args.extend(ids);
    match run("docker", &args) {
        Ok(output) => DockerResult::Output(output),
        Err(error) => DockerResult::Error(error),
    }
}

fn containers(result: &DockerResult) -> Vec<Container> {
    match result {
        DockerResult::Output(output) => parse::containers(output),
        DockerResult::Error(error) => {
            let (status, detail) = match error {
                CommandError::Missing => (
                    Status::Unknown,
                    "docker is not installed on this host".to_owned(),
                ),
                CommandError::Failed => {
                    (Status::Down, "docker daemon is not responding".to_owned())
                }
            };
            vec![Container {
                name: "docker".to_owned(),
                status,
                detail,
            }]
        }
    }
}

/// Write via a temporary file and rename so the web server never serves a
/// half-written document.
///
/// # Errors
///
/// Returns any I/O error from writing or renaming the file.
pub fn write_snapshot(path: &Path, snapshot: &Snapshot) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(snapshot).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, json)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests;
