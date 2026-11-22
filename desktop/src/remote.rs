use std::io::{self, Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use orkai::pwa::crypto::{self, Channel, Ephemeral, Secret, Side};
use orkai::pwa::protocol::{Greeting, PhoneMsg, VERSION};
use orkai::pwa::server::{OUTDATED, Outbox, REPLACED, UNPAIRED, is_tick};
use orkai::util::lock;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::{Deserialize, Serialize};
use tungstenite::http::header::LOCATION;
use tungstenite::http::{Response, Uri};
use tungstenite::protocol::{CloseFrame, WebSocket};
use tungstenite::{ClientRequestBuilder, HandshakeError, Message};

pub const RETRY: [Duration; 5] = [
    Duration::from_millis(400),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];
pub const RETRY_MAX: Duration = Duration::from_secs(10);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
const TICK: Duration = Duration::from_millis(25);
const SILENCE_LIMIT: Duration = Duration::from_secs(70);
const MAX_MOVES: usize = 3;
const SECURE_SCHEME: &str = "https";
const LOCAL_SCHEME: &str = "http";
const SECURE_PORT: u16 = 443;
const LOCAL_PORT: u16 = 80;
const SOCKET_PATH: &str = "/ws";
const LINK_KEY: &str = "k";
const LOCALHOST: &str = "localhost";
const MODEL: &str = "Octo desktop";
const WORKING: &str = "working";
const PAUSED: &str = "paused";

#[cfg(windows)]
const PLATFORM: &str = "Windows";
#[cfg(target_os = "macos")]
const PLATFORM: &str = "Macintosh";
#[cfg(not(any(windows, target_os = "macos")))]
const PLATFORM: &str = "Linux";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    secure: bool,
    authority: String,
}

impl Address {
    pub fn parse(url: &str) -> Option<Self> {
        let (scheme, rest) = url.trim().split_once("://")?;
        let secure = match scheme.to_ascii_lowercase().as_str() {
            SECURE_SCHEME => true,
            LOCAL_SCHEME => false,
            _ => return None,
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = rest[..end].to_ascii_lowercase();
        if authority.is_empty() || authority.contains(['@', ' ']) || authority.chars().any(char::is_control) {
            return None;
        }
        let address = Self { secure, authority };
        (address.secure || address.is_loopback()).then_some(address)
    }

    pub fn url(&self) -> String {
        format!("{}://{}", if self.secure { SECURE_SCHEME } else { LOCAL_SCHEME }, self.authority)
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    fn socket_url(&self) -> String {
        format!("{}://{}{SOCKET_PATH}", if self.secure { "wss" } else { "ws" }, self.authority)
    }

    fn host(&self) -> &str {
        if let Some(inside) = self.authority.strip_prefix('[') {
            return inside.split(']').next().unwrap_or(inside);
        }
        match self.authority.rsplit_once(':') {
            Some((host, port)) if port.parse::<u16>().is_ok() => host,
            _ => &self.authority,
        }
    }

    fn port(&self) -> u16 {
        let fallback = if self.secure { SECURE_PORT } else { LOCAL_PORT };
        self.authority.rsplit_once(':').and_then(|(_, port)| port.parse().ok()).unwrap_or(fallback)
    }

    fn is_loopback(&self) -> bool {
        let host = self.host();
        host == LOCALHOST || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }
}

pub fn parse_link(text: &str) -> Option<(Address, Secret)> {
    let link: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let (origin, fragment) = link.split_once('#')?;
    let address = Address::parse(origin)?;
    let key = fragment
        .split('&')
        .find_map(|pair| pair.split_once('=').filter(|(name, _)| *name == LINK_KEY).map(|(_, value)| value))
        .unwrap_or(fragment);
    Some((address, crypto::decode_secret(key)?))
}

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Connecting,
    Live,
    Offline,
    Unpaired,
    Outdated,
    Replaced,
    Installing,
    Password,
    Unsupported,
    Changed,
}

impl Phase {
    pub fn stopped(self) -> bool {
        matches!(
            self,
            Phase::Unpaired | Phase::Outdated | Phase::Replaced | Phase::Password | Phase::Unsupported | Phase::Changed
        )
    }

    fn of_close(frame: Option<&CloseFrame>) -> Self {
        match frame.map(|frame| u16::from(frame.code)) {
            Some(UNPAIRED) => Phase::Unpaired,
            Some(OUTDATED) => Phase::Outdated,
            Some(REPLACED) => Phase::Replaced,
            _ => Phase::Offline,
        }
    }
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    pub note: Option<String>,
    pub host: Option<String>,
    pub version: Option<String>,
    pub agents: usize,
    pub working: usize,
    pub waiting: usize,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Notice<'a> {
    Link { phase: Phase, note: Option<&'a str> },
}

#[derive(Deserialize)]
struct Seen {
    #[serde(default)]
    state: String,
    #[serde(default)]
    starting: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Heard {
    Welcome {
        host: String,
        version: String,
    },
    State {
        #[serde(default)]
        agents: Vec<Seen>,
    },
    Usage,
    #[serde(other)]
    Other,
}

#[derive(Default)]
struct Shared {
    status: Status,
    welcome: Option<Vec<u8>>,
    state: Option<Vec<u8>>,
    usage: Option<Vec<u8>>,
    window: Option<Box<dyn Outbox + Send>>,
}

impl Shared {
    fn show(&mut self, plain: &[u8]) {
        if self.window.as_mut().is_some_and(|window| window.deliver(plain).is_err()) {
            self.window = None;
        }
    }

    fn enter(&mut self, phase: Phase) {
        self.explain(phase, None);
    }

    fn explain(&mut self, phase: Phase, note: Option<String>) {
        if self.status.phase == phase && self.status.note == note {
            return;
        }
        self.status.phase = phase;
        self.status.note = note;
        self.announce();
    }

    fn announce(&mut self) {
        let notice = Notice::Link { phase: self.status.phase, note: self.status.note.as_deref() };
        if let Ok(notice) = serde_json::to_vec(&notice) {
            self.show(&notice);
        }
    }

    fn hear(&mut self, plain: &[u8]) {
        match serde_json::from_slice::<Heard>(plain).unwrap_or(Heard::Other) {
            Heard::Welcome { host, version } => {
                self.status.host = Some(host);
                self.status.version = Some(version);
                self.welcome = Some(plain.to_vec());
                self.enter(Phase::Live);
            }
            Heard::State { agents } => {
                self.status.agents = agents.len();
                self.status.working = agents.iter().filter(|agent| agent.starting || agent.state == WORKING).count();
                self.status.waiting = agents.iter().filter(|agent| !agent.starting && agent.state == PAUSED).count();
                self.state = Some(plain.to_vec());
            }
            Heard::Usage => self.usage = Some(plain.to_vec()),
            Heard::Other => {}
        }
        self.show(plain);
    }

    fn replay(&mut self) {
        self.announce();
        for kept in [self.welcome.clone(), self.state.clone(), self.usage.clone()].into_iter().flatten() {
            self.show(&kept);
        }
    }
}

pub type Target = (Address, Secret);

enum Command {
    Send(Vec<u8>),
    Retry,
    Aim(Target),
    Park(Phase, Option<String>),
}

enum Ended {
    Dropped,
    Closed(Phase),
    Aimed(Target),
    Parked(Phase, Option<String>),
}

#[derive(Clone)]
pub struct Remote {
    shared: Arc<Mutex<Shared>>,
    commands: Sender<Command>,
}

impl Remote {
    pub fn start(address: Address, secret: Secret, client: String) -> io::Result<Self> {
        Self::spawn(Some((address, secret)), client)
    }

    pub fn parked(client: String) -> io::Result<Self> {
        Self::spawn(None, client)
    }

    fn spawn(target: Option<Target>, client: String) -> io::Result<Self> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (commands, inbox) = mpsc::channel();
        let seen = Arc::clone(&shared);
        thread::Builder::new()
            .name("orkai-desktop-remote".into())
            .spawn(move || keep_linked(target, &client, &seen, &inbox))?;
        Ok(Self { shared, commands })
    }

    pub fn aim(&self, address: Address, secret: Secret) {
        let _ = self.commands.send(Command::Aim((address, secret)));
    }

    pub fn park(&self, phase: Phase, note: Option<String>) {
        let _ = self.commands.send(Command::Park(phase, note));
    }

    pub fn status(&self) -> Status {
        lock(&self.shared).status.clone()
    }

    pub fn watch(&self, window: Box<dyn Outbox + Send>) {
        let mut shared = lock(&self.shared);
        shared.window = Some(window);
        shared.replay();
        drop(shared);
        let _ = self.send(&PhoneMsg::Sync);
    }

    pub fn unwatch(&self) {
        let mut shared = lock(&self.shared);
        if shared.window.take().is_some() {
            drop(shared);
            let _ = self.send(&PhoneMsg::Close);
        }
    }

    pub fn send(&self, message: &PhoneMsg) -> Result<(), String> {
        let plain = serde_json::to_vec(message).map_err(|err| err.to_string())?;
        self.commands.send(Command::Send(plain)).map_err(|_| "Octo stopped talking to this device".to_string())
    }

    pub fn retry(&self) {
        let _ = self.commands.send(Command::Retry);
    }
}

fn keep_linked(mut target: Option<Target>, client: &str, shared: &Arc<Mutex<Shared>>, commands: &Receiver<Command>) {
    let mut attempt = 0;
    loop {
        let ended = match &target {
            Some((address, secret)) => {
                lock(shared).enter(Phase::Connecting);
                match Session::open(address, secret, client) {
                    Ok(mut session) => {
                        let ended = session.run(shared, commands);
                        if session.welcomed {
                            attempt = 0;
                        }
                        ended
                    }
                    Err(phase) => Ended::Closed(phase),
                }
            }
            None => idle(commands),
        };
        let ended = match ended {
            Ended::Closed(phase) => {
                lock(shared).enter(phase);
                rest(commands, phase, &mut attempt)
            }
            other => other,
        };
        match ended {
            Ended::Dropped => return,
            Ended::Closed(_) => {}
            Ended::Aimed(next) => {
                target = Some(next);
                attempt = 0;
            }
            Ended::Parked(phase, note) => {
                target = None;
                lock(shared).explain(phase, note);
            }
        }
    }
}

fn idle(commands: &Receiver<Command>) -> Ended {
    loop {
        match commands.recv() {
            Ok(Command::Aim(target)) => return Ended::Aimed(target),
            Ok(Command::Park(phase, note)) => return Ended::Parked(phase, note),
            Ok(Command::Send(_) | Command::Retry) => {}
            Err(_) => return Ended::Dropped,
        }
    }
}

fn rest(commands: &Receiver<Command>, phase: Phase, attempt: &mut usize) -> Ended {
    let delay = RETRY.get(*attempt).copied().unwrap_or(RETRY_MAX);
    let until = Instant::now() + delay;
    loop {
        let heard = if phase.stopped() {
            commands.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            commands.recv_timeout(until.saturating_duration_since(Instant::now()))
        };
        match heard {
            Ok(Command::Retry) => {
                *attempt = 0;
                return Ended::Closed(phase);
            }
            Ok(Command::Aim(target)) => return Ended::Aimed(target),
            Ok(Command::Park(phase, note)) => return Ended::Parked(phase, note),
            Ok(Command::Send(_)) => {}
            Err(RecvTimeoutError::Timeout) => {
                *attempt += 1;
                return Ended::Closed(phase);
            }
            Err(RecvTimeoutError::Disconnected) => return Ended::Dropped,
        }
    }
}

enum Wire {
    Plain(TcpStream),
    Secure(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Wire {
    fn socket(&self) -> &TcpStream {
        match self {
            Wire::Plain(stream) => stream,
            Wire::Secure(stream) => &stream.sock,
        }
    }
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Wire::Plain(stream) => stream.read(buf),
            Wire::Secure(stream) => stream.read(buf),
        }
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Wire::Plain(stream) => stream.write(buf),
            Wire::Secure(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Wire::Plain(stream) => stream.flush(),
            Wire::Secure(stream) => stream.flush(),
        }
    }
}

fn tls() -> io::Result<Arc<ClientConfig>> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    if let Some(config) = CONFIG.get() {
        return Ok(Arc::clone(config));
    }
    let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::clone(CONFIG.get_or_init(|| Arc::new(config))))
}

fn dial(address: &Address) -> io::Result<Wire> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "the address of this device did not resolve");
    for target in (address.host(), address.port()).to_socket_addrs()? {
        match TcpStream::connect_timeout(&target, DIAL_TIMEOUT) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
                stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
                if !address.secure {
                    return Ok(Wire::Plain(stream));
                }
                let name = ServerName::try_from(address.host().to_string()).map_err(io::Error::other)?;
                let session = ClientConnection::new(tls()?, name).map_err(io::Error::other)?;
                return Ok(Wire::Secure(Box::new(StreamOwned::new(session, stream))));
            }
            Err(err) => last = err,
        }
    }
    Err(last)
}

fn user_agent() -> String {
    format!("Octo/{} ({PLATFORM})", env!("CARGO_PKG_VERSION"))
}

fn moved(from: &Address, response: &Response<Option<Vec<u8>>>) -> Option<Address> {
    if !response.status().is_redirection() {
        return None;
    }
    let next = Address::parse(response.headers().get(LOCATION)?.to_str().ok()?)?;
    (next.secure || !from.secure).then_some(next)
}

fn shake(address: &Address) -> Result<WebSocket<Wire>, Phase> {
    let mut target = address.clone();
    for _ in 0..=MAX_MOVES {
        let wire = dial(&target).map_err(|_| Phase::Offline)?;
        let uri = target.socket_url().parse::<Uri>().map_err(|_| Phase::Offline)?;
        let request = ClientRequestBuilder::new(uri).with_header("User-Agent", user_agent());
        match tungstenite::client(request, wire) {
            Ok((ws, _)) => return Ok(ws),
            Err(HandshakeError::Failure(tungstenite::Error::Http(response))) => {
                target = moved(&target, &response).ok_or(Phase::Offline)?;
            }
            Err(_) => return Err(Phase::Offline),
        }
    }
    Err(Phase::Offline)
}

struct Session {
    ws: WebSocket<Wire>,
    channel: Channel,
    heard: Instant,
    welcomed: bool,
}

impl Session {
    fn open(address: &Address, secret: &Secret, client: &str) -> Result<Self, Phase> {
        let mut ws = shake(address)?;
        let own = Ephemeral::generate().map_err(|_| Phase::Offline)?;
        let hello = Greeting::Hello { version: VERSION, key: crypto::encode(own.public_key()) };
        let hello = serde_json::to_string(&hello).map_err(|_| Phase::Offline)?;
        ws.send(Message::text(hello)).map_err(|_| Phase::Offline)?;
        let peer = match greeting(&mut ws)? {
            Greeting::Hello { version: VERSION, key } => crypto::decode_public_key(&key).ok_or(Phase::Outdated)?,
            _ => return Err(Phase::Outdated),
        };
        let channel = Channel::establish(Side::Phone, own, &peer, secret).map_err(|_| Phase::Offline)?;
        let mut session = Self { ws, channel, heard: Instant::now(), welcomed: false };
        let device = PhoneMsg::Device {
            model: Some(MODEL.into()),
            os_version: None,
            standalone: true,
            client: Some(client.to_string()),
            push: None,
        };
        for message in [PhoneMsg::Sync, device] {
            let plain = serde_json::to_vec(&message).map_err(|_| Phase::Offline)?;
            session.deliver(&plain).map_err(|_| Phase::Offline)?;
        }
        session.ws.get_ref().socket().set_read_timeout(Some(TICK)).map_err(|_| Phase::Offline)?;
        Ok(session)
    }

    fn deliver(&mut self, plain: &[u8]) -> io::Result<()> {
        let frame = self.channel.seal(plain).map_err(io::Error::other)?;
        self.ws.send(Message::binary(frame)).map_err(io::Error::other)
    }

    fn hang_up(&mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }

    fn run(&mut self, shared: &Arc<Mutex<Shared>>, commands: &Receiver<Command>) -> Ended {
        loop {
            loop {
                match commands.try_recv() {
                    Ok(Command::Send(plain)) => {
                        if self.deliver(&plain).is_err() {
                            return Ended::Closed(Phase::Offline);
                        }
                    }
                    Ok(Command::Retry) => {}
                    Ok(Command::Aim(target)) => {
                        self.hang_up();
                        return Ended::Aimed(target);
                    }
                    Ok(Command::Park(phase, note)) => {
                        self.hang_up();
                        return Ended::Parked(phase, note);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.hang_up();
                        return Ended::Dropped;
                    }
                }
            }
            if self.heard.elapsed() >= SILENCE_LIMIT {
                return Ended::Closed(Phase::Offline);
            }
            match self.ws.read() {
                Ok(Message::Binary(frame)) => {
                    self.heard = Instant::now();
                    let Ok(plain) = self.channel.open(&frame) else { return Ended::Closed(Phase::Offline) };
                    let mut shared = lock(shared);
                    shared.hear(&plain);
                    self.welcomed = self.welcomed || shared.status.phase == Phase::Live;
                }
                Ok(Message::Ping(_) | Message::Pong(_)) => self.heard = Instant::now(),
                Ok(Message::Close(frame)) => return Ended::Closed(Phase::of_close(frame.as_ref())),
                Ok(Message::Text(_) | Message::Frame(_)) => return Ended::Closed(Phase::Offline),
                Err(err) if is_tick(&err) => {}
                Err(_) => return Ended::Closed(Phase::Offline),
            }
        }
    }
}

fn greeting(ws: &mut WebSocket<Wire>) -> Result<Greeting, Phase> {
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => return serde_json::from_str(text.as_str()).map_err(|_| Phase::Outdated),
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Ok(Message::Close(frame)) => return Err(Phase::of_close(frame.as_ref())),
            Ok(Message::Binary(_)) => return Err(Phase::Outdated),
            Err(_) => return Err(Phase::Offline),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use orkai::config::Config;
    use orkai::launch::Launcher;
    use orkai::paths::Home;
    use orkai::pwa::bridge;
    use orkai::pwa::hub::Hub;
    use orkai::pwa::server::Server;
    use orkai::store::Store;
    use serde_json::Value;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(20);

    pub(crate) struct Octo {
        pub dir: PathBuf,
        pub hub: Arc<Hub>,
        pub secret: Secret,
        pub port: u16,
    }

    impl Drop for Octo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl Octo {
        pub fn link(&self) -> String {
            bridge::pairing_link(&format!("http://127.0.0.1:{}", self.port), &self.secret)
        }

        fn address(&self) -> Address {
            Address::parse(&format!("http://127.0.0.1:{}", self.port)).unwrap()
        }
    }

    pub(crate) fn other_octo(name: &str, host: &str) -> Octo {
        let dir =
            std::env::temp_dir().join(format!("octo-remote-{name}-{}-{}", std::process::id(), orkai::util::now_ms()));
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        let hub = Hub::new(launcher, host.into());
        let secret = crypto::secret().unwrap();
        let server = Server::bind(0, Arc::clone(&hub), secret).unwrap();
        let port = server.port();
        thread::spawn(move || server.serve());
        Octo { dir, hub, secret, port }
    }

    pub(crate) struct Window(pub Sender<Value>);

    impl Outbox for Window {
        fn deliver(&mut self, plain: &[u8]) -> io::Result<()> {
            self.0.send(serde_json::from_slice(plain).map_err(io::Error::other)?).map_err(io::Error::other)
        }
    }

    pub(crate) fn window() -> (Box<Window>, Receiver<Value>) {
        let (tx, rx) = mpsc::channel();
        (Box::new(Window(tx)), rx)
    }

    pub(crate) fn until(rx: &Receiver<Value>, wanted: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + TIMEOUT;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(message) if wanted(&message) => return message,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        panic!("the window never got the expected message");
    }

    fn settled(remote: &Remote, wanted: impl Fn(&Status) -> bool) -> Status {
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            let status = remote.status();
            if wanted(&status) {
                return status;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the link never got there: {:?}", remote.status());
    }

    #[test]
    fn a_pairing_link_says_where_the_other_octo_is_and_carries_its_key() {
        let secret = crypto::secret().unwrap();
        let link = bridge::pairing_link("https://Quiet-River.TryCloudflare.com", &secret);
        let (address, key) = parse_link(&format!("  {link}\n")).unwrap();
        assert_eq!(address.url(), "https://quiet-river.trycloudflare.com");
        assert_eq!(address.socket_url(), "wss://quiet-river.trycloudflare.com/ws");
        assert_eq!((address.host(), address.port()), ("quiet-river.trycloudflare.com", 443));
        assert_eq!(key, secret);
        let bare = format!("https://octo.example:8443/#{}", crypto::encode(&secret));
        let (address, key) = parse_link(&bare).unwrap();
        assert_eq!((address.host(), address.port(), key), ("octo.example", 8443, secret));
    }

    #[test]
    fn a_link_without_a_key_or_over_plain_http_to_another_machine_is_refused() {
        let key = crypto::encode(&crypto::secret().unwrap());
        assert!(parse_link("https://octo.example/").is_none());
        assert!(parse_link("https://octo.example/#k=short").is_none());
        assert!(parse_link(&format!("http://octo.example/#k={key}")).is_none(), "the key never travels in clear");
        assert!(parse_link(&format!("ftp://octo.example/#k={key}")).is_none());
        assert!(parse_link(&format!("https://user@octo.example/#k={key}")).is_none());
        assert!(parse_link(&format!("http://127.0.0.1:7470/#k={key}")).is_some());
        assert!(parse_link(&format!("http://localhost:7470/#k={key}")).is_some());
        assert!(parse_link(&format!("http://[::1]:7470/#k={key}")).is_some());
    }

    #[test]
    fn a_window_on_another_octo_is_welcomed_sees_its_agents_and_drives_it() {
        let octo = other_octo("drive", "otherhost");
        std::fs::create_dir_all(octo.dir.join("alpha")).unwrap();
        let remote = Remote::start(octo.address(), octo.secret, "desk-1".into()).unwrap();
        let (seen, rx) = window();
        remote.watch(seen);
        let first = rx.recv_timeout(TIMEOUT).unwrap();
        assert_eq!(first["type"], "link", "the window first learns how the link is doing");
        let welcome = until(&rx, |message| message["type"] == "welcome");
        assert_eq!(welcome["host"], "otherhost");
        let overview = until(&rx, |message| message["type"] == "state");
        assert_eq!(overview["host"], "otherhost");
        let status = settled(&remote, |status| status.phase == Phase::Live);
        assert_eq!(status.host.as_deref(), Some("otherhost"));
        assert_eq!(status.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));

        remote
            .send(&PhoneMsg::Folders {
                path: octo.dir.to_string_lossy().into_owned(),
                hidden: false,
                places: false,
                seq: 9,
            })
            .unwrap();
        let folders = until(&rx, |message| message["type"] == "folders");
        let names: Vec<&str> =
            folders["dirs"].as_array().unwrap().iter().filter_map(|dir| dir["name"].as_str()).collect();
        assert!(names.contains(&"alpha"), "{names:?}");
        assert_eq!(folders["seq"], 9);

        let deadline = Instant::now() + TIMEOUT;
        while octo.hub.devices().first().and_then(|device| device.model.clone()).as_deref() != Some(MODEL)
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        let device = octo.hub.devices().pop().expect("the other Octo lists this computer");
        assert_eq!(device.model.as_deref(), Some(MODEL));
        assert!(device.agent.starts_with("Octo/"), "{}", device.agent);
    }

    #[test]
    fn a_window_that_arrives_later_gets_what_the_link_already_knows() {
        let octo = other_octo("late", "latehost");
        let remote = Remote::start(octo.address(), octo.secret, "desk-1".into()).unwrap();
        settled(&remote, |status| status.phase == Phase::Live);
        let (seen, rx) = window();
        remote.watch(seen);
        let kinds: Vec<String> =
            (0..2).map(|_| rx.recv_timeout(TIMEOUT).unwrap()["type"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["link", "welcome"]);
        remote.unwatch();
        let deadline = Instant::now() + Duration::from_millis(300);
        while rx.recv_timeout(Duration::from_millis(50)).is_ok() && Instant::now() < deadline {}
        octo.hub.nudge();
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "a window that left hears nothing more");
        assert_eq!(remote.status().phase, Phase::Live, "the link stays up for the sidebar");
    }

    fn signpost(location: String, times: usize) -> Address {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = Address::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        thread::spawn(move || {
            for _ in 0..times {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut asked = [0u8; 2048];
                let _ = stream.read(&mut asked);
                let _ = write!(
                    stream,
                    "HTTP/1.1 307 Temporary Redirect
Location: {location}
Content-Length: 0
Connection: close

"
                );
            }
        });
        address
    }

    #[test]
    fn a_fixed_address_that_points_to_where_the_other_octo_is_now_is_followed() {
        let octo = other_octo("moved", "movedhost");
        let fixed = signpost(format!("http://127.0.0.1:{}/ws", octo.port), 1);
        let remote = Remote::start(fixed, octo.secret, "desk-1".into()).unwrap();
        let status = settled(&remote, |status| status.phase == Phase::Live);
        assert_eq!(status.host.as_deref(), Some("movedhost"));
    }

    #[test]
    fn a_fixed_address_never_sends_a_secure_link_to_plain_http_or_around_in_circles() {
        let secure = Address::parse("https://0123456789abcdef0123456789abcdef.octo.marujo.dev").unwrap();
        let answer = |status: u16, location: &str| {
            Response::builder().status(status).header(LOCATION, location).body(None::<Vec<u8>>).unwrap()
        };
        assert_eq!(
            moved(&secure, &answer(307, "https://quiet-river.trycloudflare.com/ws")).map(|next| next.url()).as_deref(),
            Some("https://quiet-river.trycloudflare.com")
        );
        assert_eq!(moved(&secure, &answer(307, "http://127.0.0.1:7470/ws")), None, "a secure link stays secure");
        assert_eq!(moved(&secure, &answer(307, "http://evil.example.com/ws")), None);
        assert_eq!(moved(&secure, &answer(307, "ftp://evil.example.com/ws")), None);
        assert_eq!(moved(&secure, &answer(404, "https://quiet-river.trycloudflare.com/ws")), None);

        let circle = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = circle.local_addr().unwrap().port();
        let around = format!("http://127.0.0.1:{port}/ws");
        let asked = Arc::new(Mutex::new(0usize));
        let counted = Arc::clone(&asked);
        thread::spawn(move || {
            for stream in circle.incoming() {
                let Ok(mut stream) = stream else { return };
                *lock(&counted) += 1;
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let _ = write!(
                    stream,
                    "HTTP/1.1 307 Temporary Redirect
Location: {around}
Content-Length: 0
Connection: close

"
                );
            }
        });
        let address = Address::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        assert!(matches!(shake(&address), Err(Phase::Offline)));
        assert_eq!(*lock(&asked), MAX_MOVES + 1, "a circle is given up after a few moves");
    }

    #[test]
    fn a_link_with_the_wrong_key_stops_as_unpaired_instead_of_retrying_forever() {
        let octo = other_octo("unpaired", "host");
        let remote = Remote::start(octo.address(), crypto::secret().unwrap(), "desk-1".into()).unwrap();
        let status = settled(&remote, |status| status.phase == Phase::Unpaired);
        assert_eq!(status.host, None);
        thread::sleep(Duration::from_millis(600));
        assert_eq!(remote.status().phase, Phase::Unpaired);
        assert_eq!(octo.hub.phones(), 0);
    }

    #[test]
    fn a_device_that_is_not_there_is_offline_and_tried_again() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let address = Address::parse(&format!("http://127.0.0.1:{port}")).unwrap();
        let remote = Remote::start(address, crypto::secret().unwrap(), "desk-1".into()).unwrap();
        let (seen, rx) = window();
        remote.watch(seen);
        until(&rx, |message| message["type"] == "link" && message["phase"] == "offline");
        until(&rx, |message| message["type"] == "link" && message["phase"] == "connecting");
        assert!(remote.send(&PhoneMsg::Sync).is_ok(), "what is sent while offline is dropped, not an error");
    }

    #[test]
    fn the_sidebar_counts_who_works_and_who_waits_on_the_other_octo() {
        let mut shared = Shared::default();
        shared.hear(br#"{"type":"welcome","host":"mac","version":"9.9.9"}"#);
        shared.hear(
            br#"{"type":"state","agents":[{"state":"working","starting":false},{"state":"paused","starting":false},{"state":"paused","starting":true},{"state":"done"},{"state":"new-state"}]}"#,
        );
        assert_eq!(
            shared.status,
            Status {
                phase: Phase::Live,
                note: None,
                host: Some("mac".into()),
                version: Some("9.9.9".into()),
                agents: 5,
                working: 2,
                waiting: 1
            }
        );
        shared.hear(br#"{"type":"something-new","agents":"not a list"}"#);
        assert_eq!(shared.status.agents, 5, "a message this version does not know changes nothing");
        assert_eq!(serde_json::to_value(&shared.status).unwrap()["phase"], "live");
    }

    #[test]
    fn a_closed_socket_says_why_only_for_the_codes_of_the_bridge() {
        let frame = |code: u16| CloseFrame { code: code.into(), reason: "".into() };
        assert_eq!(Phase::of_close(Some(&frame(UNPAIRED))), Phase::Unpaired);
        assert_eq!(Phase::of_close(Some(&frame(OUTDATED))), Phase::Outdated);
        assert_eq!(Phase::of_close(Some(&frame(REPLACED))), Phase::Replaced);
        assert_eq!(Phase::of_close(Some(&frame(1000))), Phase::Offline);
        assert_eq!(Phase::of_close(None), Phase::Offline);
        assert!(Phase::Unpaired.stopped() && !Phase::Offline.stopped());
    }

    #[test]
    fn retries_back_off_and_a_stopped_link_waits_to_be_asked() {
        let (tx, rx) = mpsc::channel();
        let mut attempt = RETRY.len();
        tx.send(Command::Send(Vec::new())).unwrap();
        tx.send(Command::Retry).unwrap();
        assert!(matches!(rest(&rx, Phase::Unpaired, &mut attempt), Ended::Closed(Phase::Unpaired)));
        assert_eq!(attempt, 0, "asking again starts from the short wait");
        let started = Instant::now();
        assert!(matches!(rest(&rx, Phase::Offline, &mut attempt), Ended::Closed(Phase::Offline)));
        assert!(started.elapsed() >= RETRY[0]);
        assert_eq!(attempt, 1);
        drop(tx);
        assert!(matches!(rest(&rx, Phase::Offline, &mut attempt), Ended::Dropped), "a forgotten device stops its link");
    }

    #[test]
    fn a_link_that_waits_for_its_tunnel_says_why_and_connects_once_it_is_given_where() {
        let octo = other_octo("parked", "server");
        let remote = Remote::parked("desk-1".into()).unwrap();
        let (seen, rx) = window();
        remote.watch(seen);
        remote.park(Phase::Password, Some("deploy@build".into()));
        let asked = until(&rx, |message| message["type"] == "link" && message["phase"] == "password");
        assert_eq!(asked["note"], "deploy@build");
        assert_eq!(remote.status().note.as_deref(), Some("deploy@build"));
        remote.aim(octo.address(), octo.secret);
        assert_eq!(until(&rx, |message| message["type"] == "welcome")["host"], "server");
        let status = settled(&remote, |status| status.phase == Phase::Live);
        assert_eq!(status.note, None, "a link that is up no longer carries the old reason");
        remote.park(Phase::Offline, Some("the tunnel closed".into()));
        until(&rx, |message| message["type"] == "link" && message["phase"] == "offline");
        let deadline = Instant::now() + TIMEOUT;
        while octo.hub.phones() != 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(octo.hub.phones(), 0, "a parked link hangs up");
        thread::sleep(Duration::from_millis(600));
        assert_eq!(remote.status().phase, Phase::Offline, "and does not dial again by itself");
    }
}
