//! One Envy process per user.
//!
//! The first process listens on `127.0.0.1` and writes the port next to the
//! config. A later process connects, sends its arguments (a deeplink or
//! `activate`), and exits. The listener never receives file contents.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use envy_core::config_dir;

pub struct Instance {
    listener: TcpListener,
}

pub fn claim_or_forward(payload: &str) -> Option<Instance> {
    let port_path = port_file();
    if let Some(port) = read_port(&port_path) {
        if forward(port, payload) {
            std::process::exit(0);
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    listener.set_nonblocking(true).ok()?;
    if let Ok(port) = listener.local_addr().map(|addr| addr.port()) {
        if let Some(parent) = port_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&port_path, port.to_string());
    }
    Some(Instance { listener })
}

impl Instance {
    /// Lines sent by later launches. Non-blocking.
    pub fn poll(&self) -> Vec<String> {
        let mut messages = Vec::new();
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                    let mut buf = String::new();
                    let _ = stream.read_to_string(&mut buf);
                    if let Some(line) = buf.lines().next() {
                        let line = line.trim();
                        if !line.is_empty() {
                            messages.push(line.to_string());
                        }
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        messages
    }
}

fn port_file() -> PathBuf {
    config_dir().join("instance.port")
}

fn read_port(path: &std::path::Path) -> Option<u16> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn forward(port: u16, payload: &str) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(250)) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
    let body = format!("{payload}\n");
    stream.write_all(body.as_bytes()).is_ok()
}
