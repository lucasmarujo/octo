use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::fsutil;
use crate::launch::{self, Launcher};
use crate::paths::{HOME_ENV, Home};
use crate::pwa::cloudflared;
use crate::pwa::crypto::{self, Secret};
use crate::pwa::hub::Hub;
use crate::pwa::server::Server;
use crate::pwa::tunnel::{self, Tunnel, TunnelKind};
use crate::store::Store;
use crate::util::{lock, now_ms};

pub const COMMAND: &str = "__pwa";
pub const DEFAULT_PORT: u16 = 7470;
pub const STALE_AFTER_MS: u64 = 15_000;
pub const LINK_KEY: &str = "k";
const INFO_FILE: &str = "pwa.json";
const SECRET_FILE: &str = "pwa.key";
const STOP_FILE: &str = "pwa.stop";
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
    fsutil::remove_file_if_exists(&secret_path(home))
}

pub fn start(home: &Home, exe: &Path, tunnel: TunnelKind, port: u16) -> io::Result<bool> {
    if status(home).is_some() {
        return Ok(false);
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
    let store = Store::open(home.clone())?;
    let config = Config::load_or_init(home)?;
    let exe = std::env::current_exe()?;
    let launcher = Launcher::new(store, config, &exe)?;
    let secret = load_secret(home)?;
    let host = gethostname::gethostname().to_string_lossy().to_lowercase();
    let hub = Hub::new(launcher, host);
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
    };
    write_info(home, &info)?;
    thread::Builder::new().name("orkai-pwa-accept".into()).spawn(move || server.serve())?;
    let state = Arc::new(State { home: home.clone(), info: Mutex::new(info) });
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
    };
    write_info(home, &info)?;
    Err(err)
}

struct State {
    home: Home,
    info: Mutex<Info>,
}

impl State {
    fn stop_requested(&self) -> bool {
        stop_path(&self.home).exists()
    }

    fn update(&self, change: impl FnOnce(&mut Info)) {
        let mut info = lock(&self.info);
        change(&mut info);
        info.alive_at_ms = now_ms();
        let _ = write_info(&self.home, &info);
    }

    fn beat(&self, hub: &Hub) {
        let mut written = Instant::now();
        while !self.stop_requested() {
            thread::sleep(SUPERVISE_EVERY);
            let phones = hub.phones();
            if phones != lock(&self.info).phones || written.elapsed() >= HEARTBEAT_EVERY {
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
        self.tunnel = None;
        match self.connect() {
            Ok(tunnel) => {
                let url = tunnel.url().to_string();
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

    fn connect(&self) -> io::Result<Tunnel> {
        let (home, port, kind) = {
            let info = lock(&self.state.info);
            (self.state.home.clone(), info.port, info.tunnel)
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
                tunnel::cloudflare(&program, port, &|| self.state.stop_requested())
            }
            TunnelKind::Tailscale => {
                self.enter(Phase::Connecting);
                tunnel::tailscale(port)
            }
        }
    }

    fn shut_down(&mut self) {
        if let Some(mut tunnel) = self.tunnel.take() {
            tunnel.close();
        }
        let _ = fsutil::remove_file_if_exists(&info_path(&self.state.home));
        let _ = fsutil::remove_file_if_exists(&stop_path(&self.state.home));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
