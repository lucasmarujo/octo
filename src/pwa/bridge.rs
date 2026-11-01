use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::holder::oplog;
use crate::launch;
use crate::paths::{HOME_ENV, Home};
use crate::pwa::cloudflared;
use crate::pwa::crypto::{self, Secret};
use crate::pwa::hub::Hub;
use crate::pwa::server::Server;
use crate::pwa::tunnel::{self, QuickTunnel, Tunnel, TunnelKind};
use crate::store::Store;
use crate::util::{lock, now_ms};

pub const COMMAND: &str = "__pwa";
pub const VERSION: u32 = 7;
pub const DEFAULT_PORT: u16 = 7470;
pub const STALE_AFTER_MS: u64 = 15_000;
pub const LINK_KEY: &str = "k";
const INFO_FILE: &str = "pwa.json";
const SECRET_FILE: &str = "pwa.key";
const STOP_FILE: &str = "pwa.stop";
const TUNNEL_FILE: &str = "pwa-tunnel.json";
const LOG_FILE: &str = "pwa.log";
const MAX_LOG_BYTES: u64 = 1024 * 1024;
const STOP_WAIT: Duration = Duration::from_secs(10);
const SUPERVISE_EVERY: Duration = Duration::from_millis(500);
const HEARTBEAT_EVERY: Duration = Duration::from_secs(5);
const RETRY_FIRST: Duration = Duration::from_secs(3);
const RETRY_MAX: Duration = Duration::from_secs(60);
const RIVAL_WAIT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(150);

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Installing,
    Connecting,
    Ready,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub pid: u32,
    pub port: u16,
    pub tunnel: TunnelKind,
    pub phase: Phase,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub phones: usize,
    pub started_at_ms: u64,
    pub alive_at_ms: u64,
    #[serde(default)]
    pub exe: Option<PathBuf>,
    #[serde(default)]
    pub version: u32,
}

impl Info {
    pub fn is_alive(&self, now: u64) -> bool {
        now.saturating_sub(self.alive_at_ms) < STALE_AFTER_MS
    }

    pub fn ready_url(&self) -> Option<&str> {
        self.url.as_deref().filter(|_| self.phase == Phase::Ready)
    }
}

fn info_path(home: &Home) -> PathBuf {
    home.root().join(INFO_FILE)
}

fn secret_path(home: &Home) -> PathBuf {
    home.root().join(SECRET_FILE)
}

fn stop_path(home: &Home) -> PathBuf {
    home.root().join(STOP_FILE)
}

pub fn tunnel_path(home: &Home) -> PathBuf {
    home.root().join(TUNNEL_FILE)
}

pub fn log_path(home: &Home) -> PathBuf {
    home.root().join(LOG_FILE)
}

pub fn diag(home: &Home, message: &str) {
    let path = log_path(home);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() >= MAX_LOG_BYTES) {
        let _ = fs::rename(&path, oplog::rotated_path(&path));
    }
    let line = format!("{} {}\n", now_ms(), message);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn load_tunnel(home: &Home) -> Option<QuickTunnel> {
    fsutil::read_json_opt(&tunnel_path(home)).ok().flatten()
}

fn saved_tunnel(home: &Home) -> io::Result<QuickTunnel> {
    if let Some(saved) = load_tunnel(home) {
        return Ok(saved);
    }
    let created = tunnel::create_quick_tunnel()?;
    fsutil::write_json_atomic(&tunnel_path(home), &created)?;
    diag(home, &format!("created and saved the quick tunnel {}", created.url()));
    Ok(created)
}

pub fn read_info(home: &Home) -> Option<Info> {
    fsutil::read_json_opt(&info_path(home)).ok().flatten()
}

pub fn status(home: &Home) -> Option<Info> {
    read_info(home).filter(|info| info.is_alive(now_ms()))
}

fn write_info(home: &Home, info: &Info) -> io::Result<()> {
    fsutil::write_json_atomic(&info_path(home), info)
}

pub fn pairing_link(url: &str, secret: &Secret) -> String {
    format!("{}/#{LINK_KEY}={}", url.trim_end_matches('/'), crypto::encode(secret))
}

pub fn load_secret(home: &Home) -> io::Result<Secret> {
    let path = secret_path(home);
    if let Some(secret) = std::fs::read_to_string(&path).ok().and_then(|text| crypto::decode_secret(&text)) {
        return Ok(secret);
    }
    let secret = crypto::secret()?;
    fsutil::write_atomic(&path, crypto::encode(&secret).as_bytes())?;
    Ok(secret)
}

pub fn forget_phones(home: &Home) -> io::Result<()> {
    fsutil::remove_file_if_exists(&secret_path(home))?;
    fsutil::remove_file_if_exists(&tunnel_path(home))
}

pub fn start(home: &Home, exe: &Path, tunnel: TunnelKind, port: u16) -> io::Result<bool> {
    if let Some(info) = status(home) {
        if info.version >= VERSION {
            return Ok(false);
        }
        diag(home, &format!("replacing the bridge {} of an older orkai", info.pid));
        stop(home)?;
        if !wait_until(home, STOP_WAIT, |info| info.is_none()) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "the orkai pwa of an older orkai did not stop"));
        }
    }
    fsutil::remove_file_if_exists(&stop_path(home))?;
    let mut command = Command::new(exe);
    command
        .arg(COMMAND)
        .arg("--tunnel")
        .arg(tunnel.label())
        .arg("--port")
        .arg(port.to_string())
        .env(HOME_ENV, home.root())
        .current_dir(home.root())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    launch::spawn_detached(&mut command)?;
    Ok(true)
}

pub fn stop(home: &Home) -> io::Result<()> {
    if read_info(home).is_some() {
        fsutil::write_atomic(&stop_path(home), b"")?;
    }
    Ok(())
}

pub fn wait_until(home: &Home, timeout: Duration, mut done: impl FnMut(Option<&Info>) -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if done(status(home).as_ref()) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(POLL);
    }
}

pub fn run(home: &Home, tunnel: TunnelKind, port: u16) -> io::Result<()> {
    let hub = Hub::open(home)?;
    let store = hub.store().clone();
    let exe = std::env::current_exe()?;
    let secret = load_secret(home)?;
    let server = match Server::bind(port, Arc::clone(&hub), secret) {
        Ok(server) => server,
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => return rival(home, tunnel, port, err),
        Err(err) => return Err(err),
    };
    let now = now_ms();
    let info = Info {
        pid: std::process::id(),
        port: server.port(),
        tunnel,
        phase: Phase::Connecting,
        url: None,
        error: None,
        phones: 0,
        started_at_ms: now,
        alive_at_ms: now,
        exe: Some(exe),
        version: VERSION,
    };
    write_info(home, &info)?;
    diag(home, &format!("bridge {} listens on 127.0.0.1:{} with the {} tunnel", info.pid, info.port, tunnel.label()));
    thread::Builder::new().name("orkai-pwa-accept".into()).spawn(move || server.serve())?;
    let state = Arc::new(State { store, info: Mutex::new(info) });
    let beat = Arc::clone(&state);
    thread::Builder::new().name("orkai-pwa-beat".into()).spawn(move || beat.beat(&hub))?;
    let mut supervisor = Supervisor { state, tunnel: None, retry_at: Instant::now(), backoff: RETRY_FIRST };
    supervisor.run();
    supervisor.shut_down();
    Ok(())
}

fn rival(home: &Home, tunnel: TunnelKind, port: u16, err: io::Error) -> io::Result<()> {
    if wait_until(home, RIVAL_WAIT, |info| info.is_some_and(|info| info.pid != std::process::id())) {
        return Ok(());
    }
    let now = now_ms();
    let info = Info {
        pid: std::process::id(),
        port,
        tunnel,
        phase: Phase::Failed,
        url: None,
        error: Some(format!("port {port} is in use; set pwa_port in config.json")),
        phones: 0,
        started_at_ms: now,
        alive_at_ms: now,
        exe: None,
        version: VERSION,
    };
    write_info(home, &info)?;
    diag(home, &format!("port {port} is in use by something other than the orkai pwa"));
    Err(err)
}

struct State {
    store: Store,
    info: Mutex<Info>,
}

impl State {
    fn home(&self) -> &Home {
        self.store.home()
    }

    fn turned_off(&self) -> bool {
        self.store.pwa_enabled() == Some(false)
    }

    fn stop_requested(&self) -> bool {
        stop_path(self.home()).exists() || self.turned_off()
    }

    fn update(&self, change: impl FnOnce(&mut Info)) {
        let mut info = lock(&self.info);
        change(&mut info);
        info.alive_at_ms = now_ms();
        let _ = write_info(self.home(), &info);
    }

    fn beat(&self, hub: &Hub) {
        let mut written = Instant::now();
        while !self.stop_requested() {
            thread::sleep(SUPERVISE_EVERY);
            let phones = hub.phones();
            let before = lock(&self.info).phones;
            if phones != before {
                diag(self.home(), &format!("phones connected: {phones}"));
            }
            if phones != before || written.elapsed() >= HEARTBEAT_EVERY {
                self.update(|info| info.phones = phones);
                written = Instant::now();
            }
        }
    }
}

struct Supervisor {
    state: Arc<State>,
    tunnel: Option<Tunnel>,
    retry_at: Instant,
    backoff: Duration,
}

impl Supervisor {
    fn run(&mut self) {
        while !self.state.stop_requested() {
            let down = self.tunnel.as_mut().is_none_or(|tunnel| !tunnel.is_running());
            if down && Instant::now() >= self.retry_at {
                self.open();
            }
            thread::sleep(SUPERVISE_EVERY);
        }
    }

    fn open(&mut self) {
        if let Some(tunnel) = self.tunnel.take() {
            if tunnel.rejected() {
                self.forget_tunnel(tunnel.url());
            } else {
                diag(self.state.home(), &format!("the tunnel to {} went down; reopening the same one", tunnel.url()));
            }
        }
        match self.connect() {
            Ok(tunnel) => {
                let url = tunnel.url().to_string();
                diag(self.state.home(), &format!("the {} tunnel is up at {url}", tunnel.kind().label()));
                self.backoff = RETRY_FIRST;
                self.tunnel = Some(tunnel);
                self.state.update(|info| {
                    info.url = Some(url);
                    info.error = None;
                    info.phase = Phase::Ready;
                });
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => {
                if err.kind() == io::ErrorKind::PermissionDenied {
                    let url = load_tunnel(self.state.home()).map(|saved| saved.url()).unwrap_or_default();
                    self.forget_tunnel(&url);
                }
                diag(self.state.home(), &format!("the tunnel did not open: {err}; retrying in {:?}", self.backoff));
                self.retry_at = Instant::now() + self.backoff;
                self.backoff = (self.backoff * 2).min(RETRY_MAX);
                self.state.update(|info| {
                    info.url = None;
                    info.error = Some(err.to_string());
                    info.phase = Phase::Failed;
                });
            }
        }
    }

    fn enter(&self, phase: Phase) {
        self.state.update(|info| {
            info.url = None;
            info.phase = phase;
        });
    }

    fn forget_tunnel(&self, url: &str) {
        let home = self.state.home();
        let _ = fsutil::remove_file_if_exists(&tunnel_path(home));
        diag(home, &format!("cloudflare no longer knows the tunnel {url}; a new one replaces it, scan the new code"));
    }

    fn connect(&self) -> io::Result<Tunnel> {
        let (home, port, kind) = {
            let info = lock(&self.state.info);
            (self.state.home().clone(), info.port, info.tunnel)
        };
        match kind {
            TunnelKind::Cloudflare => {
                let program = match cloudflared::locate(&home) {
                    Some(program) => program,
                    None => {
                        self.enter(Phase::Installing);
                        cloudflared::install(&home)?
                    }
                };
                self.enter(Phase::Connecting);
                let saved = saved_tunnel(&home)?;
                tunnel::cloudflare(&home, &program, port, &saved, &|| self.state.stop_requested())
            }
            TunnelKind::Tailscale => {
                self.enter(Phase::Connecting);
                tunnel::tailscale(port)
            }
        }
    }

    fn shut_down(&mut self) {
        let home = self.state.home();
        let reason = if self.state.turned_off() { "the pwa was turned off" } else { "it was asked to stop" };
        diag(home, &format!("bridge {} stops: {reason}", std::process::id()));
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.close();
        }
        let _ = fsutil::remove_file_if_exists(&info_path(home));
        let _ = fsutil::remove_file_if_exists(&stop_path(home));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UiState;

    fn home() -> Home {
        let dir = std::env::temp_dir().join(format!(
            "orkai-pwa-bridge-{}-{}-{}",
            std::process::id(),
            now_ms(),
            crate::util::to_hex(&crate::util::random_bytes::<4>().unwrap())
        ));
        let home = Home::at(&dir);
        home.ensure().unwrap();
        home
    }

    fn info(alive_at_ms: u64) -> Info {
        Info {
            pid: 1,
            port: DEFAULT_PORT,
            tunnel: TunnelKind::Cloudflare,
            phase: Phase::Ready,
            url: Some("https://a-b.trycloudflare.com".into()),
            error: None,
            phones: 0,
            started_at_ms: 1,
            alive_at_ms,
            exe: None,
            version: VERSION,
        }
    }

    #[test]
    fn the_pairing_secret_survives_restarts_until_the_phones_are_forgotten() {
        let home = home();
        let first = load_secret(&home).unwrap();
        assert_eq!(load_secret(&home).unwrap(), first);
        forget_phones(&home).unwrap();
        assert_ne!(load_secret(&home).unwrap(), first);
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn the_secret_rides_in_the_fragment_of_the_link() {
        let secret = [9u8; crypto::SECRET_LEN];
        let link = pairing_link("https://a-b.trycloudflare.com/", &secret);
        let (base, fragment) = link.split_once('#').unwrap();
        assert_eq!(base, "https://a-b.trycloudflare.com/");
        let value = fragment.strip_prefix(&format!("{LINK_KEY}=")).unwrap();
        assert_eq!(crypto::decode_secret(value), Some(secret));
    }

    #[test]
    fn a_bridge_that_stopped_beating_is_not_running() {
        let home = home();
        assert!(status(&home).is_none());
        write_info(&home, &info(now_ms())).unwrap();
        assert!(status(&home).is_some());
        write_info(&home, &info(now_ms() - STALE_AFTER_MS - 1)).unwrap();
        assert!(read_info(&home).is_some());
        assert!(status(&home).is_none(), "a stale heartbeat means the bridge is gone");
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn only_a_ready_bridge_has_a_url_to_pair() {
        let mut info = info(now_ms());
        assert_eq!(info.ready_url(), Some("https://a-b.trycloudflare.com"));
        info.phase = Phase::Connecting;
        assert_eq!(info.ready_url(), None);
    }

    #[test]
    fn stopping_asks_a_running_bridge_and_ignores_a_missing_one() {
        let home = home();
        stop(&home).unwrap();
        assert!(!stop_path(&home).exists(), "nothing to stop");
        write_info(&home, &info(now_ms())).unwrap();
        stop(&home).unwrap();
        assert!(stop_path(&home).exists());
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn the_bridge_stops_as_soon_as_any_orkai_turns_the_pwa_off() {
        let home = home();
        let store = Store::open(home.clone()).unwrap();
        let state = State { store: store.clone(), info: Mutex::new(info(now_ms())) };
        assert!(!state.stop_requested(), "no ui state says nothing");
        store.save_ui(&UiState { pwa: true, ..Default::default() }).unwrap();
        assert!(!state.stop_requested());
        store.save_ui(&UiState { pwa: false, ..Default::default() }).unwrap();
        assert!(state.stop_requested(), "off in ui.json is off for the bridge too");
        store.save_ui(&UiState { pwa: true, ..Default::default() }).unwrap();
        fsutil::write_atomic(&stop_path(&home), b"").unwrap();
        assert!(state.stop_requested());
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn starting_keeps_a_current_bridge_and_replaces_one_of_an_older_orkai() {
        let home = home();
        let missing = home.root().join("missing-orkai");
        write_info(&home, &info(now_ms())).unwrap();
        assert!(!start(&home, &missing, TunnelKind::Cloudflare, DEFAULT_PORT).unwrap());
        assert!(!stop_path(&home).exists(), "a current bridge is left alone");

        write_info(&home, &Info { version: VERSION - 1, ..info(now_ms()) }).unwrap();
        let old = home.clone();
        let old_bridge = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !stop_path(&old).exists() {
                if Instant::now() >= deadline {
                    return false;
                }
                thread::sleep(Duration::from_millis(10));
            }
            fsutil::remove_file_if_exists(&info_path(&old)).unwrap();
            true
        });
        let err = start(&home, &missing, TunnelKind::Cloudflare, DEFAULT_PORT).unwrap_err();
        assert!(old_bridge.join().unwrap(), "the older bridge was asked to stop");
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "then the new one was launched: {err}");
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn a_new_code_forgets_the_phones_and_the_tunnel() {
        let home = home();
        let secret = load_secret(&home).unwrap();
        let saved = QuickTunnel {
            id: "9d03eb81-8e8c-4b1d-80b0-ca686c0ef16b".into(),
            account_tag: "5ab4e9dfbd435d24068829fda0077963".into(),
            secret: "c2VjcmV0".into(),
            hostname: "boost-rid-constant-tmp.trycloudflare.com".into(),
        };
        fsutil::write_json_atomic(&tunnel_path(&home), &saved).unwrap();
        assert_eq!(saved_tunnel(&home).unwrap(), saved, "a saved tunnel is reused, so its link never changes");
        forget_phones(&home).unwrap();
        assert!(load_tunnel(&home).is_none());
        assert_ne!(load_secret(&home).unwrap(), secret);
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn the_log_keeps_one_older_file_when_it_grows() {
        let home = home();
        diag(&home, "bridge 1 listens");
        let log = std::fs::read_to_string(log_path(&home)).unwrap();
        assert!(log.ends_with(" bridge 1 listens\n"), "{log}");
        std::fs::write(log_path(&home), vec![b'x'; usize::try_from(MAX_LOG_BYTES).unwrap()]).unwrap();
        diag(&home, "bridge 2 listens");
        let rotated = oplog::rotated_path(&log_path(&home));
        assert_eq!(std::fs::metadata(rotated).unwrap().len(), MAX_LOG_BYTES);
        let log = std::fs::read_to_string(log_path(&home)).unwrap();
        assert!(log.ends_with(" bridge 2 listens\n") && log.lines().count() == 1, "{log}");
        std::fs::remove_dir_all(home.root()).unwrap();
    }
}
