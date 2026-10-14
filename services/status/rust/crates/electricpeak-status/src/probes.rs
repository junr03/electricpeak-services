//! Things that touch the host: spawn tools and open a socket.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::Status;

pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

pub enum CommandError {
    /// The program is not installed on this host.
    Missing,
    Failed,
}

pub fn run(program: &str, args: &[&str]) -> Result<String, CommandError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => CommandError::Missing,
            _ => CommandError::Failed,
        })?;
    let mut stdout = child.stdout.take().ok_or(CommandError::Failed)?;
    let reader = thread::spawn(move || {
        let mut buffer = String::new();
        stdout.read_to_string(&mut buffer).map(|_| buffer)
    });
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let exit = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break exit,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CommandError::Failed);
            }
            Err(_) => return Err(CommandError::Failed),
        }
    };
    let output = reader
        .join()
        .map_err(|_| CommandError::Failed)?
        .map_err(|_| CommandError::Failed)?;
    if exit.success() {
        Ok(output)
    } else {
        Err(CommandError::Failed)
    }
}

/// An SSH server speaks first, so a banner starting with `SSH-` proves sshd
/// (not just any listener) is accepting connections. The banner itself is
/// never published.
pub fn ssh(address: &str) -> (Status, String) {
    let Some(target) = address
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
    else {
        return (Status::Unknown, "invalid SSH probe address".into());
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&target, Duration::from_secs(3)) else {
        return (Status::Down, "connection refused or timed out".into());
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut banner = [0_u8; 4];
    match stream.read_exact(&mut banner) {
        Ok(()) if &banner == b"SSH-" => {
            let _ = stream.write_all(b"SSH-2.0-electricpeak-status\r\n");
            (Status::Up, "accepting SSH connections".into())
        }
        Ok(()) => (Status::Down, "port open but not speaking SSH".into()),
        Err(_) => (Status::Down, "port open but no SSH banner".into()),
    }
}
