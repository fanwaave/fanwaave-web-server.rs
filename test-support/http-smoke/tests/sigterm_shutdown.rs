#![forbid(unsafe_code)]
#![cfg(unix)]

use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn responds_to_health(addr: SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(100)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    if stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buffer = [0u8; 128];
    match stream.read(&mut buffer) {
        Ok(n) => buffer[..n].starts_with(b"HTTP/1.1 200"),
        Err(_) => false,
    }
}

#[test]
fn sigterm_shuts_down_live_server_with_success_exit() -> io::Result<()> {
    let temporary = TcpListener::bind("127.0.0.1:0")?;
    let addr = temporary.local_addr()?;
    drop(temporary);

    let child = Command::new(env!("CARGO_BIN_EXE_sigterm_probe"))
        .arg(addr.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut child = ChildGuard(child);
    let startup_deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if child.0.try_wait()?.is_some() {
            return Err(io::Error::other("signal probe exited before serving health"));
        }
        if responds_to_health(addr) {
            break;
        }
        if Instant::now() >= startup_deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "signal probe startup timed out"));
        }
        thread::sleep(Duration::from_millis(25));
    }

    let sent = Command::new("kill")
        .arg("-TERM")
        .arg(child.0.id().to_string())
        .status()?;
    if !sent.success() {
        return Err(io::Error::other("unable to send SIGTERM to signal probe"));
    }
    let shutdown_deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(exit) = child.0.try_wait()? {
            assert!(exit.success(), "SIGTERM caused an ungraceful server exit");
            return Ok(());
        }
        if Instant::now() >= shutdown_deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "SIGTERM shutdown timed out"));
        }
        thread::sleep(Duration::from_millis(25));
    }
}
