use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::launch::{self, command_output, without_console};

pub const READY_TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(200);
const TAILSCALE_TIMEOUT: Duration = Duration::from_secs(20);
const QUICK_TUNNEL_DOMAIN: &str = ".trycloudflare.com";
const QUICK_TUNNEL_API: &str = "api.trycloudflare.com";
const REGISTERED: &str = "Registered tunnel connection";
const HTTPS_PORT: &str = "443";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TunnelKind {
    #[default]
    Cloudflare,
    Tailscale,
}

impl TunnelKind {
    pub const ALL: [TunnelKind; 2] = [TunnelKind::Cloudflare, TunnelKind::Tailscale];

    pub fn label(self) -> &'static str {
        match self {
            TunnelKind::Cloudflare => "cloudflare",
            TunnelKind::Tailscale => "tailscale",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|kind| kind.label() == text)
    }
}

pub struct Tunnel {
    kind: TunnelKind,
    url: String,
    child: Option<Child>,
    serve: Option<PathBuf>,
}

impl Tunnel {
    pub fn kind(&self) -> TunnelKind {
        self.kind
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn is_running(&mut self) -> bool {
        self.child.as_mut().is_none_or(|child| matches!(child.try_wait(), Ok(None)))
    }

    pub fn close(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(program) = self.serve.take() {
            let _ = command_output(
                Command::new(program).args(["serve", &format!("--https={HTTPS_PORT}"), "off"]),
                TAILSCALE_TIMEOUT,
            );
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn origin(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn quick_tunnel_url(line: &str) -> Option<String> {
    line.split(|c: char| c.is_whitespace() || c == '|' || c == '"')
        .filter_map(|word| word.strip_prefix("https://"))
        .map(|host| host.trim_end_matches(['/', '.', ',']))
        .find(|host| {
            host.ends_with(QUICK_TUNNEL_DOMAIN)
                && *host != QUICK_TUNNEL_API
                && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        })
        .map(|host| format!("https://{host}"))
}

enum Event {
    Url(String),
    Registered,
}

pub fn cloudflare(program: &Path, port: u16, cancelled: &dyn Fn() -> bool) -> io::Result<Tunnel> {
    let mut child = without_console(&mut Command::new(program))
        .args(["tunnel", "--no-autoupdate", "--url", &origin(port)])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().ok_or_else(|| io::Error::other("cloudflared has no output"))?;
    let (tx, rx) = mpsc::channel();
    thread::Builder::new().name("orkai-pwa-cloudflared".into()).spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let event = match quick_tunnel_url(&line) {
                Some(url) => Event::Url(url),
                None if line.contains(REGISTERED) => Event::Registered,
                None => continue,
            };
            let _ = tx.send(event);
        }
    })?;
    let mut tunnel = Tunnel { kind: TunnelKind::Cloudflare, url: String::new(), child: Some(child), serve: None };
    let mut registered = false;
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "the pwa was turned off"));
        }
        if !tunnel.is_running() {
            return Err(io::Error::other("cloudflared stopped before the tunnel was up"));
        }
        match rx.recv_timeout(POLL) {
            Ok(Event::Url(url)) => tunnel.url = url,
            Ok(Event::Registered) => registered = true,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => thread::sleep(POLL),
        }
        let has_url = !tunnel.url.is_empty();
        if has_url && (registered || Instant::now() >= deadline) {
            return Ok(tunnel);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "cloudflare did not open the tunnel in time"));
        }
    }
}

pub fn tailscale_host(status: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(status).ok()?;
    if value.get("BackendState").and_then(|state| state.as_str()) != Some("Running") {
        return None;
    }
    let host = value.get("Self")?.get("DNSName")?.as_str()?.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_string())
}

pub fn tailscale(port: u16) -> io::Result<Tunnel> {
    let program = launch::resolve_program("tailscale").map_err(|_| {
        io::Error::new(io::ErrorKind::NotFound, "tailscale is not installed (https://tailscale.com/download)")
    })?;
    let status = command_output(Command::new(&program).args(["status", "--json"]), TAILSCALE_TIMEOUT)
        .ok_or_else(|| io::Error::other("tailscale did not answer"))?;
    let host =
        tailscale_host(&status).ok_or_else(|| io::Error::other("tailscale is not connected; run `tailscale up`"))?;
    command_output(
        Command::new(&program).args(["serve", "--bg", &format!("--https={HTTPS_PORT}"), &origin(port)]),
        TAILSCALE_TIMEOUT,
    )
    .ok_or_else(|| io::Error::other("tailscale serve failed; enable HTTPS for your tailnet"))?;
    Ok(Tunnel { kind: TunnelKind::Tailscale, url: format!("https://{host}"), child: None, serve: Some(program) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_kinds_parse_from_their_labels() {
        for kind in TunnelKind::ALL {
            assert_eq!(TunnelKind::parse(kind.label()), Some(kind));
        }
        assert_eq!(TunnelKind::parse(" Tailscale "), Some(TunnelKind::Tailscale));
        assert_eq!(TunnelKind::parse("ngrok"), None);
        assert_eq!(TunnelKind::default(), TunnelKind::Cloudflare);
    }

    #[test]
    fn the_quick_tunnel_url_is_read_from_the_cloudflared_banner() {
        let banner =
            "2026-10-22T10:00:00Z INF |  https://quiet-river-sound-lamp.trycloudflare.com                    |";
        assert_eq!(quick_tunnel_url(banner).as_deref(), Some("https://quiet-river-sound-lamp.trycloudflare.com"));
    }

    #[test]
    fn other_urls_in_the_cloudflared_log_are_ignored() {
        let failure =
            r#"ERR failed to request quick Tunnel error="Post \"https://api.trycloudflare.com/tunnel\": EOF""#;
        assert_eq!(quick_tunnel_url(failure), None);
        assert_eq!(quick_tunnel_url("INF see https://developers.cloudflare.com/tunnel"), None);
        assert_eq!(quick_tunnel_url("INF https://evil.example.com?x=.trycloudflare.com"), None);
        assert_eq!(quick_tunnel_url("nothing here"), None);
    }

    #[test]
    fn the_tailscale_host_comes_from_a_running_node() {
        let status = r#"{"BackendState":"Running","Self":{"DNSName":"desk.tail1a2b.ts.net."}}"#;
        assert_eq!(tailscale_host(status).as_deref(), Some("desk.tail1a2b.ts.net"));
        assert_eq!(tailscale_host(r#"{"BackendState":"NeedsLogin","Self":{"DNSName":"desk.ts.net."}}"#), None);
        assert_eq!(tailscale_host(r#"{"BackendState":"Running","Self":{"DNSName":""}}"#), None);
        assert_eq!(tailscale_host("not json"), None);
    }

    #[test]
    fn the_origin_is_the_loopback_port() {
        assert_eq!(origin(7470), "http://127.0.0.1:7470");
    }
}
