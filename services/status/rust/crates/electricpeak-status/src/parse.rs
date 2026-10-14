//! Pure parsers for the output of the tools the collector asks.

use serde_json::Value;

use crate::model::{Container, Status};

/// `tailscale status --json`: connected when the backend is running and this
/// node has not been marked offline.
pub fn tailscale(json: &str) -> (Status, String) {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return (Status::Unknown, "unreadable tailscale status".into());
    };
    let backend = value["BackendState"].as_str().unwrap_or("unknown");
    if backend != "Running" {
        return (Status::Down, format!("tailscale is {backend}"));
    }
    if value["Self"]["Online"].as_bool() == Some(false) {
        return (
            Status::Down,
            "running but not connected to the tailnet".into(),
        );
    }
    (Status::Up, "connected to the tailnet".into())
}

/// `zerotier-cli -j info` plus `zerotier-cli -j listnetworks`.
pub fn zerotier(info: &str, networks: &str) -> (Status, String) {
    let (Ok(info), Ok(networks)) = (
        serde_json::from_str::<Value>(info),
        serde_json::from_str::<Value>(networks),
    ) else {
        return (Status::Unknown, "unreadable zerotier status".into());
    };
    if info["online"].as_bool() != Some(true) {
        return (Status::Down, "zerotier is not online".into());
    }
    let networks = networks.as_array().map_or(&[][..], Vec::as_slice);
    let ok = networks.iter().filter(|n| n["status"] == "OK").count();
    let total = networks.len();
    if total == 0 {
        (Status::Down, "online but joined to no network".into())
    } else if ok == 0 {
        (
            Status::Down,
            format!("online, 0 of {total} networks connected"),
        )
    } else if ok < total {
        (
            Status::Degraded,
            format!("online, {ok} of {total} networks connected"),
        )
    } else {
        (Status::Up, format!("connected to {ok} network(s)"))
    }
}

/// One line per container, produced by `docker inspect --format` with the
/// template in [`INSPECT_FORMAT`].
pub const INSPECT_FORMAT: &str = "{{.Name}}|{{.State.Status}}|{{.State.ExitCode}}|{{if .State.Health}}{{.State.Health.Status}}{{end}}|{{.HostConfig.RestartPolicy.Name}}";

pub fn containers(output: &str) -> Vec<Container> {
    output.lines().filter_map(container).collect()
}

fn container(line: &str) -> Option<Container> {
    let mut fields = line.trim().split('|');
    let name = fields.next()?.trim_start_matches('/');
    let state = fields.next()?;
    let exit_code = fields.next()?;
    let health = fields.next()?;
    let restart = fields.next()?;
    if name.is_empty() {
        return None;
    }
    let (status, detail) = match (state, health) {
        ("running", "unhealthy") => (
            Status::Down,
            "running but failing its health check".to_owned(),
        ),
        ("running", "starting") => (Status::Degraded, "starting up".to_owned()),
        ("running", "healthy") => (Status::Up, "healthy".to_owned()),
        ("running", _) => (Status::Up, "running".to_owned()),
        ("restarting", _) => (Status::Down, "restart loop".to_owned()),
        ("paused", _) => (Status::Degraded, "paused".to_owned()),
        ("exited" | "created", _) if exit_code == "0" && matches!(restart, "" | "no") => (
            Status::Idle,
            "finished or not started; not expected to run continuously".to_owned(),
        ),
        ("exited", _) => (Status::Down, format!("stopped with exit code {exit_code}")),
        ("created", _) => (Status::Down, "created but never started".to_owned()),
        ("dead" | "removing", _) => (Status::Down, state.to_owned()),
        _ => (Status::Unknown, state.to_owned()),
    };
    Some(Container {
        name: name.to_owned(),
        status,
        detail,
    })
}
