use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Message, WebSocket};

use orkai::client;
use orkai::config::Config;
use orkai::launch::Launcher;
use orkai::model::{AgentId, AgentKind, AgentState, LaunchSpec, NewAgent};
use orkai::paths::Home;
use orkai::protocol::{ClientMsg, HolderMsg, TermSize};
use orkai::pwa::crypto::{self, Channel, Ephemeral, Secret, Side};
use orkai::pwa::hub::{Event, Hub};
use orkai::pwa::protocol::{AgentView, BridgeMsg, Greeting, Overview, PhoneMsg, Screen, StartRequest, VERSION};
use orkai::pwa::server::{OUTDATED, Outbox, Peer, Server, UNPAIRED};
use orkai::store::Store;

const TIMEOUT: Duration = Duration::from_secs(25);
const HOST: &str = "testhost";
static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    dir: PathBuf,
    secret: Secret,
    port: u16,
    store: Store,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for record in self.store.list_agents().unwrap_or_default() {
            if let Ok(Some(runtime)) = self.store.read_runtime(record.id) {
                let _ = client::deliver(&runtime, &ClientMsg::Kill);
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "orkai-pwa-e2e-{}-{}-{}",
        std::process::id(),
        orkai::util::now_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Store::open(Home::at(dir.join("home"))).unwrap();
    let launcher = Launcher::new(store.clone(), Config::default(), Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let hub = Hub::new(launcher, HOST.to_string());
    let secret = crypto::secret().unwrap();
    let server = Server::bind(0, Arc::clone(&hub), secret).unwrap();
    let port = server.port();
    std::thread::spawn(move || server.serve());
    Fixture { dir, secret, port, store }
}

fn socket(port: u16) -> WebSocket<TcpStream> {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    tungstenite::client(format!("ws://127.0.0.1:{port}/ws"), stream).unwrap().0
}

fn hello(ws: &mut WebSocket<TcpStream>, version: u32) -> Ephemeral {
    let own = Ephemeral::generate().unwrap();
    let key = crypto::encode(own.public_key());
    ws.send(Message::text(serde_json::to_string(&Greeting::Hello { version, key }).unwrap())).unwrap();
    own
}

fn close_code(ws: &mut WebSocket<TcpStream>) -> Option<u16> {
    loop {
        match ws.read() {
            Ok(Message::Close(frame)) => return frame.map(|frame| u16::from(frame.code)),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

struct Phone {
    ws: WebSocket<TcpStream>,
    channel: Channel,
    text: Vec<String>,
}

impl Phone {
    fn pair(port: u16, secret: &Secret) -> Self {
        let mut ws = socket(port);
        let own = hello(&mut ws, VERSION);
        let Message::Text(reply) = ws.read().unwrap() else { panic!("expected the hello of the bridge") };
        let Greeting::Hello { version, key } = serde_json::from_str(reply.as_str()).unwrap() else {
            panic!("the bridge refused the hello")
        };
        assert_eq!(version, VERSION);
        let peer = crypto::decode_public_key(&key).unwrap();
        let channel = Channel::establish(Side::Phone, own, &peer, secret).unwrap();
        let mut phone = Phone { ws, channel, text: Vec::new() };
        phone.send(&PhoneMsg::Sync);
        phone
    }

    fn send(&mut self, message: &PhoneMsg) {
        let frame = self.channel.seal(&serde_json::to_vec(message).unwrap()).unwrap();
        self.ws.send(Message::binary(frame)).unwrap();
    }

    fn read(&mut self) -> BridgeMsg {
        loop {
            match self.ws.read().expect("the bridge closed the connection") {
                Message::Binary(frame) => {
                    let plain = self.channel.open(&frame).expect("a frame from the bridge did not decrypt");
                    return serde_json::from_slice(&plain).unwrap();
                }
                Message::Ping(_) | Message::Pong(_) => {}
                other => panic!("unexpected message {other:?}"),
            }
        }
    }

    fn until<T>(&mut self, mut wanted: impl FnMut(BridgeMsg) -> Option<T>) -> T {
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if let Some(found) = wanted(self.read()) {
                return found;
            }
        }
        panic!("the message did not arrive in time");
    }

    fn overview_where(&mut self, check: impl Fn(&Overview) -> bool) -> Overview {
        self.until(|message| match message {
            BridgeMsg::State(overview) if check(&overview) => Some(overview),
            _ => None,
        })
    }

    fn screen_where(&mut self, check: impl Fn(&Screen, &str) -> bool) -> Screen {
        let mut text = std::mem::take(&mut self.text);
        let found = self.until(|message| match message {
            BridgeMsg::Screen(screen) => {
                if screen.full || text.len() != usize::from(screen.rows) {
                    text = vec![String::new(); usize::from(screen.rows)];
                }
                for (row, runs) in &screen.lines {
                    if let Some(line) = text.get_mut(usize::from(*row)) {
                        *line = runs.iter().map(|run| run.0.as_str()).collect();
                    }
                }
                check(&screen, &text.join("\n")).then_some(screen)
            }
            _ => None,
        });
        self.text = text;
        found
    }
}

fn agent(overview: &Overview, id: AgentId) -> Option<&AgentView> {
    overview.agents.iter().find(|agent| agent.id == id)
}

fn running(agent: &AgentView) -> bool {
    !agent.starting && matches!(agent.state, AgentState::Idle | AgentState::Working | AgentState::Done)
}

fn add_agent(store: &Store, name: &str, cwd: PathBuf) -> AgentId {
    let draft =
        NewAgent { session: "default".into(), name: name.into(), kind: AgentKind::Terminal, cwd, task: String::new() };
    store.create_agent(draft).unwrap().id
}

fn start_terminal(fx: &Fixture, name: &str) -> AgentId {
    let (program, args) = if cfg!(windows) {
        (PathBuf::from("cmd.exe"), vec!["/q".to_string()])
    } else {
        (PathBuf::from("/bin/sh"), Vec::new())
    };
    start_program(fx, name, program, args)
}

fn start_program(fx: &Fixture, name: &str, program: PathBuf, args: Vec<String>) -> AgentId {
    let cwd = fx.dir.join(name);
    std::fs::create_dir_all(&cwd).unwrap();
    let id = add_agent(&fx.store, name, cwd.clone());
    let spec = LaunchSpec {
        kind: AgentKind::Terminal,
        program,
        args,
        cwd,
        env: vec![("TERM".into(), "xterm-256color".into())],
        env_remove: Vec::new(),
        initial_input: None,
        rows: 24,
        cols: 80,
        test: None,
    };
    fx.store.write_launch(id, &spec).unwrap();
    orkai::launch::spawn_holder(fx.store.home(), id, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    orkai::launch::wait_for_runtime(&fx.store, id, TIMEOUT).unwrap();
    id
}

fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn the_bridge_serves_the_app_and_a_health_check() {
    let fx = fixture();
    let index = http_get(fx.port, "/");
    assert!(index.starts_with("HTTP/1.1 200 OK"), "{index}");
    assert!(index.contains("<title>Orkai</title>"));
    assert!(index.contains("Content-Security-Policy: default-src 'self'"));
    assert!(http_get(fx.port, "/health").ends_with("\r\n\r\norkai"));
    assert!(http_get(fx.port, "/../home/pwa.key").starts_with("HTTP/1.1 404"));
}

#[test]
fn a_paired_phone_receives_the_workspace() {
    let fx = fixture();
    let id = add_agent(&fx.store, "worker", fx.dir.clone());
    let mut phone = Phone::pair(fx.port, &fx.secret);
    let BridgeMsg::Welcome { host, .. } = phone.read() else { panic!("the bridge did not welcome the phone") };
    assert_eq!(host, HOST);
    let overview = phone.overview_where(|overview| !overview.agents.is_empty());
    assert_eq!(overview.host, HOST);
    assert!(overview.kinds.contains(&AgentKind::Terminal));
    assert_eq!(overview.sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["default"]);
    let worker = agent(&overview, id).unwrap();
    assert_eq!(worker.name, "worker");
    assert!(worker.starting, "a fresh record that is not running yet is starting");
    assert!(overview.now > 0);
}

#[test]
fn a_phone_without_the_secret_is_turned_away() {
    let fx = fixture();
    let mut phone_ws = socket(fx.port);
    let own = hello(&mut phone_ws, VERSION);
    let Message::Text(reply) = phone_ws.read().unwrap() else { panic!("expected the hello") };
    let Greeting::Hello { key, .. } = serde_json::from_str(reply.as_str()).unwrap() else { panic!() };
    let wrong = crypto::secret().unwrap();
    let mut channel = Channel::establish(Side::Phone, own, &crypto::decode_public_key(&key).unwrap(), &wrong).unwrap();
    let frame = channel.seal(br#"{"type":"sync"}"#).unwrap();
    phone_ws.send(Message::binary(frame)).unwrap();
    assert_eq!(close_code(&mut phone_ws), Some(UNPAIRED));
}

#[test]
fn an_outdated_phone_is_asked_to_reload() {
    let fx = fixture();
    let mut ws = socket(fx.port);
    hello(&mut ws, VERSION - 1);
    assert_eq!(close_code(&mut ws), Some(OUTDATED));
    assert_eq!(CloseCode::from(OUTDATED), CloseCode::Library(OUTDATED));
}

#[test]
fn a_tampered_frame_ends_the_session() {
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.read();
    let mut frame = phone.channel.seal(br#"{"type":"sync"}"#).unwrap();
    frame[0] ^= 0xff;
    phone.ws.send(Message::binary(frame)).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    let closed = loop {
        match phone.ws.read() {
            Ok(Message::Close(_)) | Err(_) => break true,
            Ok(_) if Instant::now() < deadline => {}
            Ok(_) => break false,
        }
    };
    assert!(closed, "the bridge kept a session whose frames do not authenticate");
}

#[test]
fn the_phone_watches_an_agent_and_types_into_it() {
    let fx = fixture();
    let id = start_terminal(&fx, "shell");
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|overview| agent(overview, id).is_some_and(running));
    phone.send(&PhoneMsg::Open { agent: id, rows: 30, cols: 60 });
    phone.screen_where(|screen, _| screen.full && screen.agent == id);
    phone.send(&PhoneMsg::Send { text: "echo orkai-pwa-$((40+2))".into() });
    let marker = if cfg!(windows) { "orkai-pwa-$((40+2))" } else { "orkai-pwa-42" };
    phone.screen_where(|_, text| text.lines().any(|line| line.trim() == marker));
}

#[test]
fn the_phone_lists_the_agent_typed_into_last_first() {
    let fx = fixture();
    let older = start_terminal(&fx, "older");
    let newer = start_terminal(&fx, "newer");
    let order = |overview: &Overview| overview.agents.iter().map(|agent| agent.id).collect::<Vec<_>>();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|overview| overview.agents.iter().all(running) && order(overview) == [newer, older]);
    phone.send(&PhoneMsg::Open { agent: older, rows: 30, cols: 60 });
    phone.screen_where(|screen, _| screen.full && screen.agent == older);
    phone.send(&PhoneMsg::Keys { data: "x".into() });
    phone.overview_where(|overview| order(overview) == [older, newer]);
}

const FULL_SCREEN_PS1: &str = r#"
Add-Type -Namespace Orkai -Name Console -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern IntPtr GetStdHandle(int handle);
[DllImport("kernel32.dll")] public static extern bool SetConsoleMode(IntPtr handle, int mode);
'@
[void][Orkai.Console]::SetConsoleMode([Orkai.Console]::GetStdHandle(-10), 0x200)
[void][Orkai.Console]::SetConsoleMode([Orkai.Console]::GetStdHandle(-11), 0x7)
$e = [char]27
[Console]::Write("$e[?1049h$e[?1002h$e[?1006hwheel-ready`r`n")
while ($true) { [Console]::Write(([string][char][Console]::In.Read()).Replace("$e", "`r`nESC")) }
"#;

const FULL_SCREEN_SH: &str = r#"
stty raw -echo
printf '\033[?1049h\033[?1002h\033[?1006hwheel-ready\r\n'
esc=$(printf '\033')
while :; do
  c=$(dd bs=1 count=1 2>/dev/null)
  if [ "$c" = "$esc" ]; then printf '\r\nESC'; else printf '%s' "$c"; fi
done
"#;

fn full_screen_echo(dir: &Path) -> (PathBuf, Vec<String>) {
    if cfg!(windows) {
        let script = dir.join("full-screen.ps1");
        std::fs::write(&script, FULL_SCREEN_PS1).unwrap();
        let args = ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].map(String::from);
        (PathBuf::from("powershell.exe"), [args.to_vec(), vec![script.to_string_lossy().into_owned()]].concat())
    } else {
        (PathBuf::from("/bin/sh"), vec!["-c".to_string(), FULL_SCREEN_SH.to_string()])
    }
}

#[test]
fn the_phone_scrolls_a_full_screen_agent_with_the_mouse_wheel() {
    let fx = fixture();
    let (program, args) = full_screen_echo(&fx.dir);
    let id = start_program(&fx, "full-screen", program, args);
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.send(&PhoneMsg::Open { agent: id, rows: 30, cols: 50 });
    phone.screen_where(|screen, text| (screen.rows, screen.cols) == (30, 50) && text.contains("wheel-ready"));
    phone.send(&PhoneMsg::Scroll { lines: 6 });
    let up = phone.screen_where(|_, text| text.matches("ESC[<64;26;16M").count() == 2);
    assert_eq!(up.scroll, 0, "the agent scrolls itself; the phone keeps showing it live");
    phone.send(&PhoneMsg::Scroll { lines: -3 });
    phone.screen_where(|_, text| text.contains("ESC[<65;26;16M"));
}

#[test]
fn a_phone_alone_on_an_agent_gets_a_screen_its_size() {
    let fx = fixture();
    let id = start_terminal(&fx, "alone");
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.send(&PhoneMsg::Open { agent: id, rows: 30, cols: 50 });
    let screen = phone.screen_where(|screen, _| (screen.rows, screen.cols) == (30, 50));
    assert_eq!(screen.agent, id);
    phone.send(&PhoneMsg::Size { rows: 26, cols: 44 });
    phone.screen_where(|screen, _| (screen.rows, screen.cols) == (26, 44));
}

#[test]
fn the_phone_gets_its_size_when_it_opens_an_agent_the_desk_shows_and_whoever_types_last_keeps_it() {
    let fx = fixture();
    let id = start_terminal(&fx, "shared");
    let runtime = fx.store.read_runtime(id).unwrap().unwrap();
    let (desk, mut reader, _) = client::connect(&runtime).unwrap();
    let desk_size = TermSize::new(40, 120);
    desk.send(&ClientMsg::Attach(desk_size)).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(HolderMsg::Snapshot(snapshot)) = HolderMsg::read(&mut reader) {
            assert_eq!(snapshot.size, desk_size);
            break;
        }
    }
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.send(&PhoneMsg::Open { agent: id, rows: 30, cols: 50 });
    let screen = phone.screen_where(|screen, _| screen.full);
    assert_eq!((screen.rows, screen.cols), (30, 50), "the phone must not get the screen of the desk");
    desk.send(&ClientMsg::Input(b"\r".to_vec())).unwrap();
    phone.screen_where(|screen, _| (screen.rows, screen.cols) == (40, 120));
    phone.send(&PhoneMsg::Size { rows: 30, cols: 50 });
    phone.screen_where(|screen, _| (screen.rows, screen.cols) == (30, 50));
    desk.send(&ClientMsg::Input(b"\r".to_vec())).unwrap();
    phone.screen_where(|screen, _| (screen.rows, screen.cols) == (40, 120));
    drop(desk);
    phone.screen_where(|screen, _| (screen.rows, screen.cols) == (30, 50));
}

#[test]
fn the_phone_starts_stops_and_removes_agents() {
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|_| true);
    let cwd = fx.dir.join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    phone.send(&PhoneMsg::Start(StartRequest {
        kind: AgentKind::Terminal,
        session: "default".into(),
        cwd: cwd.to_string_lossy().into_owned(),
        task: String::new(),
        worktree: false,
        bypass: false,
    }));
    let started = phone.overview_where(|overview| overview.agents.iter().any(running));
    let id = started.agents[0].id;
    assert_eq!(started.agents[0].name, "term-1");
    assert_eq!(started.agents[0].kind, AgentKind::Terminal);
    phone.send(&PhoneMsg::Stop { agent: id });
    phone.overview_where(|overview| agent(overview, id).is_some_and(|a| a.state == AgentState::Exited));
    phone.send(&PhoneMsg::Remove { agent: id });
    phone.overview_where(|overview| agent(overview, id).is_none());
    assert!(fx.store.load_agent(id).is_err(), "the record is gone");
}

#[test]
fn a_bad_request_from_the_phone_gets_a_notice_and_changes_nothing() {
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|_| true);
    phone.send(&PhoneMsg::Start(StartRequest {
        kind: AgentKind::Terminal,
        session: "default".into(),
        cwd: fx.dir.join("missing").to_string_lossy().into_owned(),
        task: String::new(),
        worktree: false,
        bypass: false,
    }));
    let text = phone.until(|message| match message {
        BridgeMsg::Notice { text, .. } => Some(text),
        _ => None,
    });
    assert!(text.contains("is not a folder"), "{text}");
    assert!(fx.store.list_agents().unwrap().is_empty());
}

#[test]
fn the_phone_creates_sessions_and_browses_folders() {
    let fx = fixture();
    std::fs::create_dir_all(fx.dir.join("alpha")).unwrap();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.send(&PhoneMsg::Session { name: "mobile".into() });
    phone.overview_where(|overview| overview.sessions.iter().any(|s| s.name == "mobile"));
    phone.send(&PhoneMsg::Folders { path: fx.dir.to_string_lossy().into_owned() });
    let names = phone.until(|message| match message {
        BridgeMsg::Folders { dirs, .. } => Some(dirs.into_iter().map(|dir| dir.name).collect::<Vec<_>>()),
        _ => None,
    });
    assert!(names.contains(&"alpha".to_string()), "{names:?}");
}

#[test]
fn every_paired_phone_sees_the_same_workspace_live() {
    let fx = fixture();
    let mut first = Phone::pair(fx.port, &fx.secret);
    let mut second = Phone::pair(fx.port, &fx.secret);
    first.overview_where(|_| true);
    second.overview_where(|_| true);
    let id = add_agent(&fx.store, "shared-view", fx.dir.clone());
    first.overview_where(|overview| agent(overview, id).is_some());
    second.overview_where(|overview| agent(overview, id).is_some());
}

struct Window(Vec<BridgeMsg>);

impl Outbox for Window {
    fn deliver(&mut self, plain: &[u8]) -> std::io::Result<()> {
        self.0.push(serde_json::from_slice(plain).map_err(std::io::Error::other)?);
        Ok(())
    }
}

fn pump_until<T>(peer: &mut Peer, window: &mut Window, mut wanted: impl FnMut(&BridgeMsg) -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        peer.wait(window).unwrap();
        if let Some(found) = window.0.drain(..).find_map(|message| wanted(&message)) {
            return found;
        }
    }
    panic!("the message did not arrive in time");
}

#[test]
fn a_window_without_a_socket_drives_the_hub_through_its_inbox() {
    let fx = fixture();
    let launcher = Launcher::new(fx.store.clone(), Config::default(), Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let hub = Hub::new(launcher, HOST.to_string());
    let mut peer = Peer::join(&hub);
    let mut window = Window(Vec::new());
    let host = pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::State(overview) => Some(overview.host.clone()),
        _ => None,
    });
    assert_eq!(host, HOST);
    assert_eq!(hub.phones(), 1);
    let cwd = fx.dir.join("window");
    std::fs::create_dir_all(&cwd).unwrap();
    let start = PhoneMsg::Start(StartRequest {
        kind: AgentKind::Terminal,
        session: "default".into(),
        cwd: cwd.to_string_lossy().into_owned(),
        task: String::new(),
        worktree: false,
        bypass: false,
    });
    peer.inbox().send(Event::Inbound(start)).unwrap();
    let started = pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::State(overview) => {
            overview.agents.iter().find(|agent| running(agent)).map(|agent| agent.name.clone())
        }
        _ => None,
    });
    assert_eq!(started, "term-1");
    peer.handle(PhoneMsg::Folders { path: fx.dir.to_string_lossy().into_owned() }, &mut window).unwrap();
    let names = pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::Folders { dirs, .. } => Some(dirs.iter().map(|dir| dir.name.clone()).collect::<Vec<_>>()),
        _ => None,
    });
    assert!(names.contains(&"window".to_string()), "{names:?}");
    drop(peer);
    assert_eq!(hub.phones(), 0, "a closed window leaves the hub");
}

#[test]
fn a_paste_types_the_text_and_leaves_the_enter_to_the_user() {
    let fx = fixture();
    let id = start_terminal(&fx, "paste");
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|overview| agent(overview, id).is_some_and(running));
    phone.send(&PhoneMsg::Open { agent: id, rows: 30, cols: 70 });
    phone.screen_where(|screen, _| screen.full && screen.agent == id);
    phone.send(&PhoneMsg::Paste { text: "echo orkai-paste-$((40+2))".into() });
    phone.screen_where(|_, text| text.contains("echo orkai-paste-$((40+2))"));
    std::thread::sleep(Duration::from_millis(1500));
    phone.send(&PhoneMsg::Keys { data: "x".into() });
    phone.screen_where(|_, text| text.contains("echo orkai-paste-$((40+2))x"));
    phone.send(&PhoneMsg::Keys { data: "\r".into() });
    let marker = if cfg!(windows) { "orkai-paste-$((40+2))x" } else { "orkai-paste-42x" };
    phone.screen_where(|_, text| text.lines().any(|line| line.trim() == marker));
}

#[test]
fn keys_named_by_the_browser_are_typed_into_the_agent() {
    let fx = fixture();
    let id = start_terminal(&fx, "keys");
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|overview| agent(overview, id).is_some_and(running));
    phone.send(&PhoneMsg::Open { agent: id, rows: 30, cols: 70 });
    phone.screen_where(|screen, _| screen.full && screen.agent == id);
    let press = |key: &str| PhoneMsg::Key { key: key.into(), ctrl: false, alt: false, shift: false };
    for key in ["e", "c", "h", "o", " ", "o", "r", "k", "a", "i", "-", "k", "e", "y", "s", "Enter"] {
        phone.send(&press(key));
    }
    phone.screen_where(|_, text| text.lines().any(|line| line.trim() == "orkai-keys"));
}

#[test]
fn a_peek_shows_the_last_lines_of_each_running_agent() {
    let fx = fixture();
    let id = start_terminal(&fx, "peek");
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|overview| agent(overview, id).is_some_and(running));
    phone.send(&PhoneMsg::Open { agent: id, rows: 30, cols: 70 });
    phone.screen_where(|screen, _| screen.full && screen.agent == id);
    phone.send(&PhoneMsg::Keys { data: "echo orkai-peek\r".into() });
    phone.screen_where(|_, text| text.lines().filter(|line| line.contains("orkai-peek")).count() >= 2);
    phone.send(&PhoneMsg::Close);
    phone.send(&PhoneMsg::Peek { agents: vec![id, id + 100], rows: 4 });
    let previews = phone.until(|message| match message {
        BridgeMsg::Peek { previews } => Some(previews),
        _ => None,
    });
    assert_eq!(previews.len(), 1, "an agent that is not running has no preview");
    assert_eq!(previews[0].agent, id);
    assert!(previews[0].lines.len() <= 4);
    let text: String = previews[0].lines.iter().flatten().map(|run| run.0.as_str()).collect();
    assert!(text.contains("orkai-peek"), "{text}");
}

#[test]
fn new_settings_reach_the_window_without_restarting_the_hub() {
    let fx = fixture();
    let launcher = Launcher::new(fx.store.clone(), Config::default(), Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let hub = Hub::new(launcher, HOST.to_string());
    let mut peer = Peer::join(&hub);
    let mut window = Window(Vec::new());
    let installed = |claude: &str| Config { claude_command: claude.to_string(), ..Config::default() };
    hub.reconfigure(installed(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    assert_eq!(hub.launcher().config().claude_command, env!("CARGO_BIN_EXE_orkai"));
    pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::State(overview) if overview.kinds.contains(&AgentKind::Claude) => Some(()),
        _ => None,
    });
    hub.reconfigure(installed("orkai-no-such-program")).unwrap();
    assert!(!hub.kinds().contains(&AgentKind::Claude));
    let kinds = pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::State(overview) if !overview.kinds.contains(&AgentKind::Claude) => Some(overview.kinds.clone()),
        _ => None,
    });
    assert!(kinds.contains(&AgentKind::Terminal));
}
