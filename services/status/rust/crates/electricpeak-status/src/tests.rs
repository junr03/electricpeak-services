use std::io::Write;
use std::net::TcpListener;
use std::thread;

use super::*;
use crate::parse;

#[test]
fn tailscale_states() {
    let up = r#"{"BackendState":"Running","Self":{"Online":true}}"#;
    assert_eq!(parse::tailscale(up).0, Status::Up);
    let offline = r#"{"BackendState":"Running","Self":{"Online":false}}"#;
    assert_eq!(parse::tailscale(offline).0, Status::Down);
    let login = r#"{"BackendState":"NeedsLogin"}"#;
    assert_eq!(
        parse::tailscale(login),
        (Status::Down, "tailscale is NeedsLogin".into())
    );
    assert_eq!(parse::tailscale("nope").0, Status::Unknown);
}

#[test]
fn zerotier_states() {
    let online = r#"{"online":true,"address":"aaaaaaaaaa"}"#;
    let ok = r#"[{"status":"OK"}]"#;
    assert_eq!(parse::zerotier(online, ok).0, Status::Up);
    assert_eq!(
        parse::zerotier(
            online,
            r#"[{"status":"OK"},{"status":"REQUESTING_CONFIGURATION"}]"#
        )
        .0,
        Status::Degraded
    );
    assert_eq!(
        parse::zerotier(online, r#"[{"status":"ACCESS_DENIED"}]"#).0,
        Status::Down
    );
    assert_eq!(parse::zerotier(online, "[]").0, Status::Down);
    assert_eq!(parse::zerotier(r#"{"online":false}"#, ok).0, Status::Down);
    assert_eq!(parse::zerotier("x", ok).0, Status::Unknown);
}

#[test]
fn container_states() {
    let output = "\
/envoy|running|0||unless-stopped
/db|running|0|healthy|always
/api|running|0|unhealthy|always
/web|running|0|starting|always
/loop|restarting|1||always
/crashed|exited|137||unless-stopped
/stopped-clean|exited|0||unless-stopped
/job|exited|0||no
/paused|paused|0||always
garbage
";
    let by_name = |name: &str| {
        parse::containers(output)
            .into_iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .status
    };
    assert_eq!(parse::containers(output).len(), 9);
    assert_eq!(by_name("envoy"), Status::Up);
    assert_eq!(by_name("db"), Status::Up);
    assert_eq!(by_name("api"), Status::Down);
    assert_eq!(by_name("web"), Status::Degraded);
    assert_eq!(by_name("loop"), Status::Down);
    assert_eq!(by_name("crashed"), Status::Down);
    assert_eq!(by_name("stopped-clean"), Status::Down);
    assert_eq!(by_name("job"), Status::Idle);
    assert_eq!(by_name("paused"), Status::Degraded);
}

#[test]
fn overall_reflects_worst_state() {
    let up = || Check::new("a", "A", Status::Up, "");
    let container = |status| Container {
        name: "c".into(),
        status,
        detail: String::new(),
    };
    assert_eq!(
        Snapshot::new(
            0,
            vec![up()],
            vec![container(Status::Up), container(Status::Idle)]
        )
        .overall,
        Overall::Operational
    );
    assert_eq!(
        Snapshot::new(0, vec![up()], vec![container(Status::Down)]).overall,
        Overall::Degraded
    );
    assert_eq!(
        Snapshot::new(0, vec![Check::new("a", "A", Status::Unknown, "")], vec![]).overall,
        Overall::Degraded
    );
    assert_eq!(
        Snapshot::new(0, vec![Check::new("a", "A", Status::Down, "")], vec![]).overall,
        Overall::Outage
    );
}

#[test]
fn ssh_probe_requires_a_banner() {
    let serve = |banner: &'static [u8]| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(banner);
                thread::sleep(std::time::Duration::from_millis(200));
            }
        });
        address
    };
    assert_eq!(
        probes::ssh(&serve(b"SSH-2.0-OpenSSH_9.9\r\n")).0,
        Status::Up
    );
    assert_eq!(probes::ssh(&serve(b"HTTP/1.1 400 Bad\r\n")).0, Status::Down);
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = closed.local_addr().unwrap().to_string();
    drop(closed);
    assert_eq!(probes::ssh(&address).0, Status::Down);
}

#[test]
fn snapshot_is_written_atomically_and_leaks_nothing_extra() {
    let directory = std::env::temp_dir().join(format!("ep-status-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("status.json");
    let snapshot = Snapshot::new(42, vec![Check::new("ssh", "SSH", Status::Up, "ok")], vec![]);
    write_snapshot(&path, &snapshot).unwrap();
    let read: Snapshot = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(read, snapshot);
    assert!(!path.with_extension("json.tmp").exists());
    fs::remove_dir_all(directory).unwrap();
}
