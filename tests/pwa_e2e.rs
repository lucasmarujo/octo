use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
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
use orkai::pwa::protocol::{
    AgentView, BridgeMsg, Details, Greeting, Overview, PhoneMsg, Push, Screen, StartRequest, Tone, VERSION,
};
use orkai::pwa::push;
use orkai::pwa::server::{OUTDATED, Outbox, Peer, REPLACED, Server, UNPAIRED};
use orkai::store::Store;

const TIMEOUT: Duration = Duration::from_secs(25);
const HOST: &str = "testhost";
static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    dir: PathBuf,
    secret: Secret,
    port: u16,
    store: Store,
    hub: Arc<Hub>,
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
    Fixture { dir, secret, port, store, hub }
}

#[test]
fn a_paired_phone_reports_its_device_to_the_desktop() {
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.send(&PhoneMsg::Device {
        model: Some("Pixel 8".into()),
        os_version: Some("14".into()),
        standalone: true,
        client: Some("tab-1".into()),
        push: None,
    });
    let deadline = Instant::now() + TIMEOUT;
    while fx.hub.devices().first().and_then(|device| device.model.as_deref()) != Some("Pixel 8")
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let device = fx.hub.devices().pop().expect("the paired phone is listed");
    assert_eq!(device.model.as_deref(), Some("Pixel 8"));
    assert_eq!(device.os_version.as_deref(), Some("14"));
    assert!(device.standalone);
    assert!(device.since_ms > 0);
}

#[test]
fn a_phone_app_that_allows_notifications_is_kept_for_the_alerts_until_it_turns_them_off() {
    let fx = fixture();
    let home = fx.store.home().clone();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    let device = |push: Option<Option<Push>>| PhoneMsg::Device {
        model: Some("Pixel 8".into()),
        os_version: Some("14".into()),
        standalone: true,
        client: Some("app-1".into()),
        push,
    };
    let kept = |want: usize| {
        let deadline = Instant::now() + TIMEOUT;
        while push::phones(&home).len() != want && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        push::phones(&home)
    };
    phone.send(&device(Some(Some(Push { token: "ExponentPushToken[abc]".into(), device: "d1".into() }))));
    let phones = kept(1);
    assert_eq!(phones.len(), 1);
    assert_eq!(
        (phones[0].client.as_str(), phones[0].token.as_str(), phones[0].device.as_str()),
        ("app-1", "ExponentPushToken[abc]", "d1")
    );
    phone.send(&device(None));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(push::phones(&home).len(), 1, "a phone that does not know yet keeps what it said before");
    phone.send(&device(Some(None)));
    assert!(kept(0).is_empty(), "a phone that turned notifications off is forgotten");
}

#[test]
fn a_tab_replaced_by_another_tab_of_the_same_browser_is_told_so_and_the_new_one_stays() {
    let fx = fixture();
    let tab = |model: &str| PhoneMsg::Device {
        model: Some(model.into()),
        os_version: None,
        standalone: false,
        client: Some("tab-1".into()),
        push: None,
    };
    let listed = |model: &str| {
        let deadline = Instant::now() + TIMEOUT;
        while fx.hub.devices().iter().all(|device| device.model.as_deref() != Some(model)) && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let mut first = Phone::pair(fx.port, &fx.secret);
    first.send(&tab("first"));
    listed("first");
    let mut second = Phone::pair(fx.port, &fx.secret);
    second.send(&tab("second"));
    listed("second");
    assert_eq!(close_code(&mut first.ws), Some(REPLACED), "the old tab learns why it was closed");
    assert_eq!(fx.hub.phones(), 1);
    second.send(&PhoneMsg::Sync);
    second.until(|message| matches!(message, BridgeMsg::State(_)).then_some(()));
    let models: Vec<_> = fx.hub.devices().into_iter().filter_map(|device| device.model).collect();
    assert_eq!(models, ["second"]);
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
    assert!(index.contains("<title>Octo</title>"));
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
        remote_control: false,
        model: None,
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
        remote_control: false,
        model: None,
    }));
    let text = phone.until(|message| match message {
        BridgeMsg::Notice { text, .. } => Some(text),
        _ => None,
    });
    assert!(text.contains("is not a folder"), "{text}");
    assert!(fx.store.list_agents().unwrap().is_empty());
}

#[test]
fn a_running_agent_moves_to_another_session_without_noticing() {
    let fx = fixture();
    fx.store.create_session("work").unwrap();
    let id = start_terminal(&fx, "shell");
    let holder = fx.store.read_runtime(id).unwrap().unwrap();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|overview| agent(overview, id).is_some_and(running));
    phone.send(&PhoneMsg::Open { agent: id, rows: 30, cols: 60 });
    phone.screen_where(|screen, _| screen.full && screen.agent == id);

    phone.send(&PhoneMsg::MoveAgent { agent: id, session: "work".into() });
    let overview = phone.overview_where(|overview| agent(overview, id).is_some_and(|agent| agent.session == "work"));
    assert!(running(agent(&overview, id).unwrap()));
    assert_eq!(overview.sessions.iter().find(|s| s.name == "work").map(|s| s.agents), Some(1));
    assert_eq!(overview.sessions.iter().find(|s| s.name == "default").map(|s| s.agents), Some(0));
    assert_eq!(fx.store.load_agent(id).unwrap().session, "work");
    assert_eq!(fx.store.read_runtime(id).unwrap().unwrap(), holder, "the same holder keeps running");
    assert_eq!(fx.store.read_exit(id).unwrap(), None);

    phone.send(&PhoneMsg::Send { text: "echo moved-$((40+2))".into() });
    let marker = if cfg!(windows) { "moved-$((40+2))" } else { "moved-42" };
    phone.screen_where(|_, text| text.lines().any(|line| line.trim() == marker));

    phone.send(&PhoneMsg::MoveAgent { agent: id, session: "nowhere".into() });
    let text = phone.until(|message| match message {
        BridgeMsg::Notice { text, tone: Tone::Error } => Some(text),
        _ => None,
    });
    assert!(text.contains("could not move the agent") && text.contains("nowhere"), "{text}");
    assert_eq!(fx.store.load_agent(id).unwrap().session, "work");
}

#[test]
fn the_phone_creates_sessions_and_browses_folders() {
    let fx = fixture();
    std::fs::create_dir_all(fx.dir.join("alpha")).unwrap();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.send(&PhoneMsg::Session { name: "mobile".into() });
    phone.overview_where(|overview| overview.sessions.iter().any(|s| s.name == "mobile"));
    phone.send(&PhoneMsg::Folders {
        path: fx.dir.to_string_lossy().into_owned(),
        hidden: false,
        places: false,
        seq: 0,
    });
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
        remote_control: false,
        model: None,
    });
    peer.inbox().send(Event::Inbound(start)).unwrap();
    let started = pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::State(overview) => {
            overview.agents.iter().find(|agent| running(agent)).map(|agent| agent.name.clone())
        }
        _ => None,
    });
    assert_eq!(started, "term-1");
    peer.handle(
        PhoneMsg::Folders { path: fx.dir.to_string_lossy().into_owned(), hidden: false, places: false, seq: 0 },
        &mut window,
    )
    .unwrap();
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

#[test]
fn the_details_of_an_agent_and_its_restart_come_from_the_core() {
    let fx = fixture();
    let id = start_terminal(&fx, "details");
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|overview| agent(overview, id).is_some_and(running));
    phone.send(&PhoneMsg::Details { agent: id });
    let details = phone.until(|message| match message {
        BridgeMsg::Details(details) => Some(details),
        _ => None,
    });
    assert_eq!(details.agent, id);
    assert!(details.subagents.is_empty() && details.overlaps.is_empty());
    assert_eq!(details.changes, None, "a terminal outside a git repository has no changes to show");
    assert_eq!(details.commits, None, "nor commits");
    phone.send(&PhoneMsg::CrossReview { agent: id, reviewer: String::new() });
    let refused = phone.until(|message| match message {
        BridgeMsg::Notice { text, tone: Tone::Error } => Some(text),
        _ => None,
    });
    assert!(refused.contains("no worktree"), "{refused}");
    phone.send(&PhoneMsg::Restart { agent: id });
    let busy = phone.until(|message| match message {
        BridgeMsg::Notice { text, tone: Tone::Error } => Some(text),
        _ => None,
    });
    assert!(busy.contains("still running"), "{busy}");
    phone.send(&PhoneMsg::Stop { agent: id });
    phone.overview_where(|overview| agent(overview, id).is_some_and(|a| a.state == AgentState::Exited));
    phone.send(&PhoneMsg::Restart { agent: id });
    phone.overview_where(|overview| agent(overview, id).is_some_and(running));
}

#[test]
fn sessions_are_renamed_and_deleted_and_trust_needs_a_repository() {
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.send(&PhoneMsg::Session { name: "nightly".into() });
    phone.overview_where(|overview| overview.sessions.iter().any(|session| session.name == "nightly"));
    phone.send(&PhoneMsg::RenameSession { name: "nightly".into(), to: "weekly".into() });
    phone.overview_where(|overview| {
        overview.sessions.iter().any(|session| session.name == "weekly")
            && !overview.sessions.iter().any(|session| session.name == "nightly")
    });
    phone.send(&PhoneMsg::RenameSession { name: "default".into(), to: "home".into() });
    phone.overview_where(|overview| {
        overview.sessions.iter().any(|session| session.name == "home")
            && !overview.sessions.iter().any(|session| session.name == "default")
            && overview.session == "home"
    });
    phone.send(&PhoneMsg::DeleteSession { name: "home".into() });
    phone.overview_where(|overview| {
        overview.sessions.iter().map(|session| session.name.as_str()).collect::<Vec<_>>() == ["weekly"]
            && overview.session == "weekly"
    });
    phone.send(&PhoneMsg::DeleteSession { name: "weekly".into() });
    phone.overview_where(|overview| {
        overview.sessions.iter().map(|session| session.name.as_str()).collect::<Vec<_>>() == ["default"]
            && overview.session == "default"
    });
    let plain = fx.dir.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    phone.send(&PhoneMsg::Trust { cwd: plain.to_string_lossy().into_owned(), command: None });
    phone.until(|message| match message {
        BridgeMsg::Notice { tone: Tone::Error, .. } => Some(()),
        _ => None,
    });
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git").args(args).current_dir(dir).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn details_of(phone: &mut Phone, id: AgentId) -> Details {
    phone.send(&PhoneMsg::Details { agent: id });
    phone.until(|message| match message {
        BridgeMsg::Details(details) if details.agent == id => Some(details),
        _ => None,
    })
}

#[test]
fn an_agent_outside_a_worktree_shows_diffs_and_commits_what_is_pending_in_its_folder() {
    let fx = fixture();
    let repo = fx.dir.join("folder");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@orkai.dev"]);
    git(&repo, &["config", "user.name", "orkai test"]);
    git(&repo, &["config", "core.autocrlf", "false"]);
    std::fs::write(repo.join("cart.js"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "start"]);
    let id = add_agent(&fx.store, "folder", repo.clone());
    let mut phone = Phone::pair(fx.port, &fx.secret);
    phone.overview_where(|overview| agent(overview, id).is_some());
    let clean = details_of(&mut phone, id);
    assert_eq!(clean.changes, Some(Vec::new()), "a clean folder has nothing pending");
    let commits = clean.commits.expect("a folder inside git shows its commits");
    assert_eq!(commits.iter().map(|commit| commit.subject.as_str()).collect::<Vec<_>>(), ["start"]);
    assert_eq!(commits[0].hash, git_output(&repo, &["rev-parse", "--short", "HEAD"]));
    assert_eq!(commits[0].author, "orkai test");
    assert!(!commits[0].on_branch);

    std::fs::write(repo.join("cart.js"), "one\ntwo\n").unwrap();
    std::fs::write(repo.join("tax.js"), "tax\n").unwrap();
    let details = details_of(&mut phone, id);
    let files: Vec<(String, usize, usize)> =
        details.changes.unwrap().into_iter().map(|file| (file.path, file.insertions, file.deletions)).collect();
    assert_eq!(files, [("cart.js".to_string(), 1, 0), ("tax.js".to_string(), 1, 0)]);
    assert!(details.overlaps.is_empty());

    phone.send(&PhoneMsg::Diff { agent: id });
    let (branch, text) = phone.until(|message| match message {
        BridgeMsg::Diff { agent, branch, text, .. } if agent == id => Some((branch, text)),
        _ => None,
    });
    assert_eq!(branch, git_output(&repo, &["branch", "--show-current"]));
    assert!(text.contains("cart.js") && text.contains("two") && text.contains("tax.js"), "{text}");

    phone.send(&PhoneMsg::Merge { agent: id });
    let done = phone.until(|message| match message {
        BridgeMsg::Notice { text, tone: Tone::Success } => Some(text),
        _ => None,
    });
    let tip = git_output(&repo, &["rev-parse", "--short", "HEAD"]);
    assert_eq!(done, format!("committed to {branch} as {tip}"));
    assert_eq!(git_output(&repo, &["log", "-1", "--format=%s"]), "folder");
    assert!(git_output(&repo, &["status", "--porcelain"]).is_empty());
    let committed = details_of(&mut phone, id);
    assert_eq!(committed.changes, Some(Vec::new()));
    let latest: Vec<String> = committed.commits.unwrap().into_iter().map(|commit| commit.subject).collect();
    assert_eq!(latest, ["folder", "start"], "the commit Octo made shows up first, like in git log");

    phone.send(&PhoneMsg::Merge { agent: id });
    let nothing = phone.until(|message| match message {
        BridgeMsg::Notice { text, tone: Tone::Info } if text.starts_with("nothing") => Some(text),
        _ => None,
    });
    assert_eq!(nothing, "nothing is pending to commit");
    assert_eq!(git_output(&repo, &["rev-parse", "--short", "HEAD"]), tip);
}

#[test]
fn the_overview_counts_the_changes_of_each_worktree() {
    let fx = fixture();
    let repo = fx.dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@orkai.dev"]);
    git(&repo, &["config", "user.name", "orkai test"]);
    std::fs::write(repo.join("cart.js"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "start"]);
    let config = Config { claude_command: env!("CARGO_BIN_EXE_orkai").to_string(), ..Config::default() };
    let launcher = Launcher::new(fx.store.clone(), config, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let hub = Hub::new(launcher, HOST.to_string());
    let mut peer = Peer::join(&hub);
    let mut window = Window(Vec::new());
    let start = PhoneMsg::Start(StartRequest {
        kind: AgentKind::Claude,
        session: "default".into(),
        cwd: repo.to_string_lossy().into_owned(),
        task: "count the changes".into(),
        worktree: true,
        bypass: false,
        remote_control: false,
        model: None,
    });
    peer.inbox().send(Event::Inbound(start)).unwrap();
    let id = pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::State(overview) => overview.agents.iter().find(|agent| agent.branch.is_some()).map(|agent| agent.id),
        _ => None,
    });
    let worktree = fx.store.load_agent(id).unwrap().worktree.unwrap();
    std::fs::write(worktree.path.join("cart.js"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(worktree.path.join("tax.js"), "tax\n").unwrap();
    let changes = pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::State(overview) => {
            agent(overview, id).and_then(|agent| agent.changes).filter(|changes| changes.files == 2)
        }
        _ => None,
    });
    assert_eq!((changes.insertions, changes.deletions), (3, 0));
}

#[test]
fn an_agent_is_never_started_twice_at_the_same_time() {
    let fx = fixture();
    let launcher = Launcher::new(fx.store.clone(), Config::default(), Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let hub = Hub::new(launcher, HOST.to_string());
    let first = hub.try_relaunch(7).expect("nothing is starting agent 7 yet");
    assert!(hub.try_relaunch(7).is_none(), "a second start waits for the first");
    assert!(hub.try_relaunch(8).is_some(), "other agents are not held back");
    drop(first);
    assert!(hub.try_relaunch(7).is_some(), "once the first start ends, the agent can start again");
}

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
const TURN_ENDED: &str = "turn-ended";
const TASK: &str = "fix the login";
const BUSY_SCREEN: &str = "Working (esc to interrupt)";
const OPENCODE_BUSY_SCREEN: &str = "esc interrupt";
const TRUST_QUESTION: &str = "Do you trust the files in this folder?";
static ONE_LAUNCH_AT_A_TIME: Mutex<()> = Mutex::new(());

const FAKE_PS1_HEAD: &str = r#"
Add-Type -Namespace Orkai -Name Console -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern IntPtr GetStdHandle(int handle);
[DllImport("kernel32.dll")] public static extern bool SetConsoleMode(IntPtr handle, int mode);
'@
[void][Orkai.Console]::SetConsoleMode([Orkai.Console]::GetStdHandle(-10), 0x200)
[void][Orkai.Console]::SetConsoleMode([Orkai.Console]::GetStdHandle(-11), 0x7)
$e = [char]27
"#;

#[derive(Clone, Copy)]
enum Fake {
    Prompt,
    ReadLine,
    Say(&'static str),
    Hook(&'static str),
    Sleep(u32),
    EndTurn,
    Hang,
    Crash,
}

fn fake_ps1(fx: &Fixture, step: Fake) -> String {
    let home = fx.store.home().root().display();
    let exe = env!("CARGO_BIN_EXE_orkai");
    let command = orkai::hooks::HOOK_COMMAND;
    let id = orkai::launch::AGENT_ID_ENV;
    match step {
        Fake::Prompt => "[Console]::Write(\"$e[2J$e[H$e[?2004h> ready`r`n\")".to_string(),
        Fake::ReadLine => "while (($c = [char][Console]::In.Read()) -ne [char]13) { [Console]::Write($c) }".to_string(),
        Fake::Say(text) => format!("[Console]::Write(\"$e[2J$e[999;1H{text}\")"),
        Fake::Hook(event) => format!(
            "$env:{}='{home}'; [IO.File]::WriteAllText(\"$PWD\\{event}.json\", '{{\"hook_event_name\":\"{event}\"}}'); cmd /c \"{exe} {command} claude %{id}% < {event}.json\"",
            orkai::paths::HOME_ENV
        ),
        Fake::Sleep(ms) => format!("Start-Sleep -Milliseconds {ms}"),
        Fake::EndTurn => format!("[void](New-Item {TURN_ENDED} -Force)"),
        Fake::Hang => "Start-Sleep -Seconds 120".to_string(),
        Fake::Crash => "exit 3".to_string(),
    }
}

fn fake_sh(fx: &Fixture, step: Fake) -> String {
    let home = fx.store.home().root().display();
    let exe = env!("CARGO_BIN_EXE_orkai");
    let command = orkai::hooks::HOOK_COMMAND;
    let id = orkai::launch::AGENT_ID_ENV;
    match step {
        Fake::Prompt => r"printf '\033[2J\033[H\033[?2004h> ready\r\n'".to_string(),
        Fake::ReadLine => "read orkai_line".to_string(),
        Fake::Say(text) => format!(r"printf '\033[2J\033[999;1H{text}'"),
        Fake::Hook(event) => format!(
            "printf '%s' '{{\"hook_event_name\":\"{event}\"}}' | {}='{home}' '{exe}' {command} claude \"${id}\"",
            orkai::paths::HOME_ENV
        ),
        Fake::Sleep(ms) => format!("sleep {}.{:03}", ms / 1_000, ms % 1_000),
        Fake::EndTurn => format!(": > {TURN_ENDED}"),
        Fake::Hang => "sleep 120".to_string(),
        Fake::Crash => "exit 3".to_string(),
    }
}

fn fake_agent(fx: &Fixture, name: &str, steps: &[Fake]) -> (String, Vec<String>) {
    if cfg!(windows) {
        let lines: Vec<String> = steps.iter().map(|step| fake_ps1(fx, *step)).collect();
        let script = fx.dir.join(format!("{name}.ps1"));
        std::fs::write(&script, format!("{FAKE_PS1_HEAD}{}\n", lines.join("\n"))).unwrap();
        let args = ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].map(String::from);
        ("powershell.exe".to_string(), [args.to_vec(), vec![script.to_string_lossy().into_owned()]].concat())
    } else {
        let lines: Vec<String> = steps.iter().map(|step| fake_sh(fx, *step)).collect();
        let script = fx.dir.join(format!("{name}.sh"));
        std::fs::write(&script, format!("{}\n", lines.join("\n"))).unwrap();
        ("/bin/sh".to_string(), vec![script.to_string_lossy().into_owned()])
    }
}

fn shell_config() -> Config {
    let (shell, args) = if cfg!(windows) { ("cmd.exe", vec!["/q".to_string()]) } else { ("/bin/sh", Vec::new()) };
    Config { shell: Some(shell.to_string()), shell_args: args, ..Config::default() }
}

fn config_running(kind: AgentKind, (command, args): (String, Vec<String>)) -> Config {
    let mut config = Config::default();
    match kind {
        AgentKind::Claude => (config.claude_command, config.claude_args) = (command, args),
        AgentKind::Kimi => (config.kimi_command, config.kimi_args) = (command, args),
        AgentKind::Opencode => (config.opencode_command, config.opencode_args) = (command, args),
        other => panic!("{other:?} is not faked here"),
    }
    config
}

fn column(starting: bool, state: AgentState) -> &'static str {
    if starting {
        return "working";
    }
    match state {
        AgentState::Paused => "needs",
        AgentState::Working => "working",
        AgentState::Done => "done",
        AgentState::Idle | AgentState::Exited | AgentState::Lost => "idle",
    }
}

struct Trace {
    started: Instant,
    seen: Vec<(u128, bool, AgentState)>,
}

impl Trace {
    fn new() -> Self {
        Self { started: Instant::now(), seen: Vec::new() }
    }

    fn note(&mut self, view: &AgentView) {
        if self.seen.last().is_none_or(|last| (last.1, last.2) != (view.starting, view.state)) {
            self.seen.push((self.started.elapsed().as_millis(), view.starting, view.state));
        }
    }

    fn columns(&self) -> Vec<&'static str> {
        let mut columns: Vec<&'static str> = self.seen.iter().map(|step| column(step.1, step.2)).collect();
        columns.dedup();
        columns
    }

    fn text(&self) -> String {
        let steps: Vec<String> = self
            .seen
            .iter()
            .map(|(at, starting, state)| {
                format!("{at}ms starting={starting} state={state:?} column={}", column(*starting, *state))
            })
            .collect();
        steps.join("; ")
    }
}

fn follow(phone: &mut Phone, dir: &str, trace: &mut Trace, done: impl Fn(&AgentView) -> bool) -> AgentId {
    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    while Instant::now() < deadline {
        if let BridgeMsg::State(overview) = phone.read()
            && let Some(view) = overview.agents.iter().find(|agent| agent.cwd.ends_with(dir))
        {
            trace.note(view);
            if done(view) {
                return view.id;
            }
        }
    }
    panic!("the agent in {dir} did not get there in time: {}", trace.text());
}

fn launch(fx: &Fixture, phone: &mut Phone, kind: AgentKind, dir: &str, task: &str, config: Config) -> PathBuf {
    let cwd = fx.dir.join(dir);
    std::fs::create_dir_all(&cwd).unwrap();
    fx.hub.reconfigure(config).unwrap();
    phone.send(&PhoneMsg::Start(StartRequest {
        kind,
        session: "default".into(),
        cwd: cwd.to_string_lossy().into_owned(),
        task: task.into(),
        worktree: false,
        bypass: false,
        remote_control: false,
        model: None,
    }));
    cwd
}

fn assert_one_turn_without_flicker(kind: AgentKind, steps: &[Fake]) {
    let _turn = orkai::util::lock(&ONE_LAUNCH_AT_A_TIME);
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    let config = config_running(kind, fake_agent(&fx, "agent", steps));
    let cwd = launch(&fx, &mut phone, kind, "turn", TASK, config);
    let mut trace = Trace::new();
    follow(&mut phone, "turn", &mut trace, |view| view.state == AgentState::Done);
    assert!(cwd.join(TURN_ENDED).exists(), "Done came before the turn ended: {}", trace.text());
    assert_eq!(trace.columns(), ["working", "done"], "{}", trace.text());
}

#[test]
fn a_launched_agent_with_hooks_stays_in_working_until_its_turn_is_done() {
    assert_one_turn_without_flicker(
        AgentKind::Claude,
        &[
            Fake::Prompt,
            Fake::ReadLine,
            Fake::Hook("UserPromptSubmit"),
            Fake::EndTurn,
            Fake::Sleep(500),
            Fake::Hook("Stop"),
            Fake::Hang,
        ],
    );
}

#[test]
fn a_launched_agent_whose_turn_ends_at_once_still_goes_from_working_to_done() {
    assert_one_turn_without_flicker(
        AgentKind::Claude,
        &[Fake::Prompt, Fake::ReadLine, Fake::EndTurn, Fake::Hook("Stop"), Fake::Hang],
    );
}

#[test]
fn a_launched_agent_without_hooks_stays_in_working_until_its_screen_settles() {
    assert_one_turn_without_flicker(
        AgentKind::Kimi,
        &[
            Fake::Prompt,
            Fake::ReadLine,
            Fake::Say(BUSY_SCREEN),
            Fake::EndTurn,
            Fake::Sleep(2_000),
            Fake::Prompt,
            Fake::Hang,
        ],
    );
}

#[test]
fn an_agent_given_its_task_in_the_arguments_stays_in_working_until_its_turn_is_done() {
    assert_one_turn_without_flicker(
        AgentKind::Opencode,
        &[
            Fake::Sleep(1_000),
            Fake::Say(OPENCODE_BUSY_SCREEN),
            Fake::EndTurn,
            Fake::Sleep(2_000),
            Fake::Prompt,
            Fake::Hang,
        ],
    );
}

#[test]
fn a_terminal_given_a_command_stays_in_working_until_the_command_settles() {
    let _turn = orkai::util::lock(&ONE_LAUNCH_AT_A_TIME);
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    launch(&fx, &mut phone, AgentKind::Terminal, "terminal", "echo hi", shell_config());
    let mut trace = Trace::new();
    follow(&mut phone, "terminal", &mut trace, |view| !view.starting && view.state == AgentState::Idle);
    assert_eq!(trace.columns(), ["working", "idle"], "{}", trace.text());
}

#[test]
fn an_agent_launched_without_a_task_goes_to_idle_and_stays_there() {
    let _turn = orkai::util::lock(&ONE_LAUNCH_AT_A_TIME);
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    let config = config_running(AgentKind::Kimi, fake_agent(&fx, "agent", &[Fake::Prompt, Fake::Hang]));
    launch(&fx, &mut phone, AgentKind::Kimi, "no-task", "", config);
    let mut trace = Trace::new();
    let id = follow(&mut phone, "no-task", &mut trace, |view| !view.starting && view.state == AgentState::Idle);
    assert_eq!(trace.columns(), ["working", "idle"], "{}", trace.text());
    std::thread::sleep(Duration::from_millis(1_000));
    assert_eq!(fx.hub.state(id), Some(AgentState::Idle), "{}", trace.text());
}

#[test]
fn a_terminal_launched_without_a_command_ends_in_idle() {
    let _turn = orkai::util::lock(&ONE_LAUNCH_AT_A_TIME);
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    launch(&fx, &mut phone, AgentKind::Terminal, "shell", "", shell_config());
    let mut trace = Trace::new();
    follow(&mut phone, "shell", &mut trace, |view| {
        !view.starting && view.state == AgentState::Idle && view.state_since != view.started_at
    });
    assert_eq!(trace.columns().last(), Some(&"idle"), "{}", trace.text());
}

#[test]
fn a_launched_agent_that_crashes_does_not_stay_in_working() {
    let _turn = orkai::util::lock(&ONE_LAUNCH_AT_A_TIME);
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    let steps = [Fake::Prompt, Fake::Sleep(300), Fake::Crash];
    let config = config_running(AgentKind::Claude, fake_agent(&fx, "agent", &steps));
    launch(&fx, &mut phone, AgentKind::Claude, "crash", TASK, config);
    let mut trace = Trace::new();
    follow(&mut phone, "crash", &mut trace, |view| view.state == AgentState::Exited);
    assert_eq!(trace.columns(), ["working", "idle"], "{}", trace.text());
}

#[test]
fn a_launched_agent_asking_something_while_it_starts_goes_to_needs_you_and_back() {
    let _turn = orkai::util::lock(&ONE_LAUNCH_AT_A_TIME);
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    let steps = [
        Fake::Say(TRUST_QUESTION),
        Fake::ReadLine,
        Fake::Prompt,
        Fake::ReadLine,
        Fake::Hook("UserPromptSubmit"),
        Fake::Sleep(500),
        Fake::EndTurn,
        Fake::Hook("Stop"),
        Fake::Hang,
    ];
    let config = config_running(AgentKind::Claude, fake_agent(&fx, "agent", &steps));
    launch(&fx, &mut phone, AgentKind::Claude, "asks", TASK, config);
    let mut trace = Trace::new();
    let id = follow(&mut phone, "asks", &mut trace, |view| view.state == AgentState::Paused);
    fx.hub.send(id, &ClientMsg::Input(b"\r".to_vec())).unwrap();
    follow(&mut phone, "asks", &mut trace, |view| view.state == AgentState::Done);
    assert_eq!(trace.columns(), ["working", "needs", "working", "done"], "{}", trace.text());
}

fn backdate(store: &Store, id: AgentId) {
    let mut record = store.load_agent(id).unwrap();
    record.created_at_ms = 1;
    store.save_agent(&record).unwrap();
}

#[test]
fn agents_found_running_when_the_app_reopens_show_their_real_state_from_the_first_message() {
    let _turn = orkai::util::lock(&ONE_LAUNCH_AT_A_TIME);
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    let idle_agent = fake_agent(&fx, "idle", &[Fake::Prompt, Fake::Hang]);
    let working_agent = fake_agent(&fx, "working", &[Fake::Say(BUSY_SCREEN), Fake::Hang]);
    launch(&fx, &mut phone, AgentKind::Kimi, "was-idle", "", config_running(AgentKind::Kimi, idle_agent));
    let mut trace = Trace::new();
    let idle = follow(&mut phone, "was-idle", &mut trace, |view| !view.starting);
    launch(&fx, &mut phone, AgentKind::Kimi, "was-working", "", config_running(AgentKind::Kimi, working_agent));
    let working = follow(&mut phone, "was-working", &mut trace, |view| view.state == AgentState::Working);
    backdate(&fx.store, idle);
    backdate(&fx.store, working);

    let launcher = Launcher::new(fx.store.clone(), Config::default(), Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let hub = Hub::new(launcher, HOST.to_string());
    let mut peer = Peer::join(&hub);
    let mut window = Window(Vec::new());
    let first = pump_until(&mut peer, &mut window, |message| match message {
        BridgeMsg::State(overview) => Some(overview.clone()),
        _ => None,
    });
    let shown = |id| agent(&first, id).map(|view| (view.starting, view.state));
    assert_eq!(shown(idle), Some((false, AgentState::Idle)), "an idle agent is not shown as starting");
    assert_eq!(shown(working), Some((false, AgentState::Working)), "a working agent is not shown as idle");
}

#[test]
fn restarting_a_stopped_agent_does_not_show_it_working_before_it_reports() {
    let _turn = orkai::util::lock(&ONE_LAUNCH_AT_A_TIME);
    let fx = fixture();
    let mut phone = Phone::pair(fx.port, &fx.secret);
    let config = config_running(AgentKind::Kimi, fake_agent(&fx, "agent", &[Fake::Prompt, Fake::Hang]));
    launch(&fx, &mut phone, AgentKind::Kimi, "restart", "", config);
    let mut trace = Trace::new();
    let id = follow(&mut phone, "restart", &mut trace, |view| !view.starting);
    phone.send(&PhoneMsg::Stop { agent: id });
    follow(&mut phone, "restart", &mut trace, |view| view.state == AgentState::Exited);
    let mut restarted = Trace::new();
    phone.send(&PhoneMsg::Restart { agent: id });
    follow(&mut phone, "restart", &mut restarted, |view| view.state == AgentState::Idle && view.started_at.is_some());
    assert!(!restarted.columns().contains(&"working"), "{}", restarted.text());
}
