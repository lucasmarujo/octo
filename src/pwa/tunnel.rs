use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::launch::{self, command_output, without_console};
use crate::paths::Home;
use crate::pwa::{bridge, cloudflared};
use crate::util::is_uuid;

pub const READY_TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(200);
const TAILSCALE_TIMEOUT: Duration = Duration::from_secs(20);
const CREATE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CREATE_BYTES: u64 = 64 * 1024;
const DNS_QUERY: &str = "https://cloudflare-dns.com/dns-query";
const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const DNS_WAIT: Duration = Duration::from_secs(30);
const DNS_POLL: Duration = Duration::from_secs(1);
const MAX_DNS_BYTES: u64 = 16 * 1024;
const DNS_NO_ERROR: u64 = 0;
const QUICK_TUNNEL_DOMAIN: &str = ".trycloudflare.com";
const QUICK_TUNNEL_API: &str = "api.trycloudflare.com";
const QUICK_TUNNEL_CREATE: &str = "https://api.trycloudflare.com/tunnel";
const REGISTERED: &str = "Registered tunnel connection";
const REJECTED: &str = "Unauthorized:";
const LOGGED_LEVELS: [&str; 2] = [" ERR ", " WRN "];
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct QuickTunnel {
    #[serde(rename = "TunnelID")]
    pub id: String,
    #[serde(rename = "AccountTag")]
    pub account_tag: String,
    #[serde(rename = "TunnelSecret")]
    pub secret: String,
    #[serde(rename = "Hostname")]
    pub hostname: String,
}

impl QuickTunnel {
    pub fn url(&self) -> String {
        format!("https://{}", self.hostname)
    }
}

#[derive(Deserialize)]
struct CreatedTunnel {
    id: String,
    hostname: String,
    account_tag: String,
    secret: String,
}

#[derive(Deserialize)]
struct Created {
    success: bool,
    result: Option<CreatedTunnel>,
}

pub fn parse_quick_tunnel(body: &[u8]) -> io::Result<QuickTunnel> {
    let invalid =
        |message: &str| io::Error::new(io::ErrorKind::InvalidData, format!("cloudflare quick tunnel: {message}"));
    let created: Created = serde_json::from_slice(body).map_err(|_| invalid("not json"))?;
    let tunnel = created.result.filter(|_| created.success).ok_or_else(|| invalid("cloudflare refused it"))?;
    if !is_uuid(&tunnel.id) {
        return Err(invalid("the tunnel id is not a uuid"));
    }
    let url = format!("https://{}", tunnel.hostname);
    if quick_tunnel_url(&url).as_deref() != Some(url.as_str()) {
        return Err(invalid("the hostname is not on trycloudflare.com"));
    }
    if tunnel.account_tag.is_empty() || tunnel.secret.is_empty() {
        return Err(invalid("no credentials came with it"));
    }
    Ok(QuickTunnel { id: tunnel.id, account_tag: tunnel.account_tag, secret: tunnel.secret, hostname: tunnel.hostname })
}

pub fn create_quick_tunnel() -> io::Result<QuickTunnel> {
    let failed = |e: ureq::Error| io::Error::other(format!("{QUICK_TUNNEL_CREATE}: {e}"));
    let mut response = cloudflared::agent(CREATE_TIMEOUT)
        .post(QUICK_TUNNEL_CREATE)
        .header("Content-Type", "application/json")
        .send_empty()
        .map_err(failed)?;
    let body = response.body_mut().with_config().limit(MAX_CREATE_BYTES).read_to_vec().map_err(failed)?;
    parse_quick_tunnel(&body)
}

pub fn resolves(answer: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(answer).is_ok_and(|answer| {
        answer.get("Status").and_then(|status| status.as_u64()) == Some(DNS_NO_ERROR)
            && answer.get("Answer").and_then(|records| records.as_array()).is_some_and(|records| !records.is_empty())
    })
}

fn published(hostname: &str) -> bool {
    cloudflared::agent(DNS_TIMEOUT)
        .get(format!("{DNS_QUERY}?name={hostname}&type=A"))
        .header("Accept", "application/dns-json")
        .call()
        .ok()
        .and_then(|mut response| response.body_mut().with_config().limit(MAX_DNS_BYTES).read_to_vec().ok())
        .is_some_and(|answer| resolves(&answer))
}

fn wait_until_published(hostname: &str, cancelled: &dyn Fn() -> bool) -> bool {
    let deadline = Instant::now() + DNS_WAIT;
    loop {
        if published(hostname) {
            return true;
        }
        if cancelled() || Instant::now() >= deadline {
            return false;
        }
        thread::sleep(DNS_POLL);
    }
}

pub struct Tunnel {
    kind: TunnelKind,
    url: String,
    child: Option<Child>,
    serve: Option<PathBuf>,
    rejected: Arc<AtomicBool>,
}

impl Tunnel {
    pub fn kind(&self) -> TunnelKind {
        self.kind
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn rejected(&self) -> bool {
        self.rejected.load(Ordering::Relaxed)
    }

    pub fn is_running(&mut self) -> bool {
        !self.rejected() && self.child.as_mut().is_none_or(|child| matches!(child.try_wait(), Ok(None)))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Registered,
    Rejected,
}

fn event(line: &str) -> Option<Event> {
    if line.contains(REJECTED) {
        Some(Event::Rejected)
    } else if line.contains(REGISTERED) {
        Some(Event::Registered)
    } else {
        None
    }
}

fn worth_logging(line: &str) -> bool {
    line.contains(REGISTERED) || LOGGED_LEVELS.iter().any(|level| line.contains(level))
}

pub fn cloudflare(
    home: &Home,
    program: &Path,
    port: u16,
    saved: &QuickTunnel,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Tunnel> {
    let mut child = without_console(&mut Command::new(program))
        .args(["tunnel", "--no-autoupdate", "run", "--credentials-file"])
        .arg(bridge::tunnel_path(home))
        .args(["--url", &origin(port), &saved.id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().ok_or_else(|| io::Error::other("cloudflared has no output"))?;
    let rejected = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (log_home, flag) = (home.clone(), Arc::clone(&rejected));
    thread::Builder::new().name("orkai-pwa-cloudflared".into()).spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if worth_logging(&line) {
                bridge::diag(&log_home, &format!("cloudflared: {line}"));
            }
            let Some(event) = event(&line) else { continue };
            if event == Event::Rejected {
                flag.store(true, Ordering::Relaxed);
            }
            let _ = tx.send(event);
        }
    })?;
    let mut tunnel =
        Tunnel { kind: TunnelKind::Cloudflare, url: saved.url(), child: Some(child), serve: None, rejected };
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "the pwa was turned off"));
        }
        if tunnel.rejected() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "cloudflare no longer knows the saved tunnel"));
        }
        if !tunnel.is_running() {
            return Err(io::Error::other("cloudflared stopped before the tunnel was up"));
        }
        match rx.recv_timeout(POLL) {
            Ok(Event::Registered) => {
                if !wait_until_published(&saved.hostname, cancelled) {
                    bridge::diag(home, &format!("cloudflare dns does not answer for {} yet", saved.hostname));
                }
                return Ok(tunnel);
            }
            Ok(Event::Rejected) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => thread::sleep(POLL),
        }
        if Instant::now() >= deadline {
            return Ok(tunnel);
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
    Ok(Tunnel {
        kind: TunnelKind::Tailscale,
        url: format!("https://{host}"),
        child: None,
        serve: Some(program),
        rejected: Arc::default(),
    })
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

    const CREATED: &str = r#"{"success":true,"result":{"id":"9d03eb81-8e8c-4b1d-80b0-ca686c0ef16b","name":"qt-rOJaK7T9","hostname":"boost-rid-constant-tmp.trycloudflare.com","account_tag":"5ab4e9dfbd435d24068829fda0077963","secret":"c2VjcmV0LW9mLXRoZS1xdWljay10dW5uZWwtdGVzdA=="},"errors":[]}"#;

    #[test]
    fn the_quick_tunnel_is_read_from_the_cloudflare_answer() {
        let tunnel = parse_quick_tunnel(CREATED.as_bytes()).unwrap();
        assert_eq!(tunnel.id, "9d03eb81-8e8c-4b1d-80b0-ca686c0ef16b");
        assert_eq!(tunnel.account_tag, "5ab4e9dfbd435d24068829fda0077963");
        assert_eq!(tunnel.secret, "c2VjcmV0LW9mLXRoZS1xdWljay10dW5uZWwtdGVzdA==");
        assert_eq!(tunnel.url(), "https://boost-rid-constant-tmp.trycloudflare.com");
    }

    #[test]
    fn a_refused_or_odd_quick_tunnel_is_never_used() {
        for body in [
            CREATED.replace(r#""success":true"#, r#""success":false"#),
            CREATED.replace("9d03eb81-8e8c-4b1d-80b0-ca686c0ef16b", "../../etc"),
            CREATED.replace("boost-rid-constant-tmp.trycloudflare.com", "evil.example.com"),
            CREATED.replace("boost-rid-constant-tmp.trycloudflare.com", "a b.trycloudflare.com"),
            CREATED.replace("5ab4e9dfbd435d24068829fda0077963", ""),
            r#"{"success":false,"result":null,"errors":[{"code":1000}]}"#.to_string(),
            "not json".to_string(),
        ] {
            let err = parse_quick_tunnel(body.as_bytes()).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{body}");
        }
    }

    #[test]
    fn the_saved_tunnel_is_a_cloudflared_credentials_file() {
        let tunnel = parse_quick_tunnel(CREATED.as_bytes()).unwrap();
        let file: serde_json::Value = serde_json::to_value(&tunnel).unwrap();
        assert_eq!(file["TunnelID"], "9d03eb81-8e8c-4b1d-80b0-ca686c0ef16b");
        assert_eq!(file["AccountTag"], "5ab4e9dfbd435d24068829fda0077963");
        assert_eq!(file["TunnelSecret"], "c2VjcmV0LW9mLXRoZS1xdWljay10dW5uZWwtdGVzdA==");
        assert_eq!(file["Hostname"], "boost-rid-constant-tmp.trycloudflare.com");
        assert_eq!(serde_json::from_value::<QuickTunnel>(file).unwrap(), tunnel);
    }

    #[test]
    fn cloudflared_tells_when_the_tunnel_is_up_or_gone() {
        let registered = "2026-10-22T23:26:14Z INF Registered tunnel connection connIndex=0 connection=0350147a event=0 ip=198.41.200.33 location=gru21 protocol=quic";
        let gone = r#"2026-10-22T23:28:39Z ERR Register tunnel error from server side error="Unauthorized: Tunnel not found" connIndex=0 event=0 ip=198.41.200.113"#;
        let offline = r#"2026-10-22T23:30:00Z ERR Failed to dial a quic connection error="failed to dial to edge with quic: timeout: no recent network activity" connIndex=0"#;
        let chatter = "2026-10-22T23:25:30Z INF Starting metrics server on 127.0.0.1:20242/metrics";
        assert_eq!(event(registered), Some(Event::Registered));
        assert_eq!(event(gone), Some(Event::Rejected));
        assert_eq!(event(offline), None, "losing the network keeps the tunnel");
        assert_eq!(event(chatter), None);
        assert!(worth_logging(registered) && worth_logging(gone) && worth_logging(offline));
        assert!(!worth_logging(chatter));
    }

    #[test]
    fn the_code_waits_until_public_dns_knows_the_tunnel() {
        let found = r#"{"Status":0,"Question":[{"name":"a-b.trycloudflare.com","type":1}],"Answer":[{"name":"a-b.trycloudflare.com","type":1,"TTL":300,"data":"104.16.230.132"}]}"#;
        let not_yet = r#"{"Status":3,"Question":[{"name":"a-b.trycloudflare.com","type":1}],"Authority":[{"name":"trycloudflare.com","type":6,"TTL":60}]}"#;
        let empty = r#"{"Status":0,"Question":[{"name":"a-b.trycloudflare.com","type":1}]}"#;
        assert!(resolves(found.as_bytes()));
        assert!(!resolves(not_yet.as_bytes()), "NXDOMAIN while cloudflare publishes the name");
        assert!(!resolves(empty.as_bytes()));
        assert!(!resolves(b"<html>blocked</html>"));
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
