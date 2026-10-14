use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use orkai::client::{self, Connection};
use orkai::holder::oplog;
use orkai::launch::{spawn_holder, wait_for_runtime};
use orkai::model::{AgentId, AgentKind, LaunchSpec, NewAgent, RuntimeInfo};
use orkai::paths::Home;
use orkai::protocol::{ClientMsg, HolderMsg, PROTOCOL_VERSION, TermSize};
use orkai::store::Store;
use orkai::util::now_ms;

const TIMEOUT: Duration = Duration::from_secs(30);

struct Fixture {
    store: Store,
    dir: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(tag: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("orkai-e2e-{tag}-{}-{}", std::process::id(), now_ms()));
    let store = Store::open(Home::at(dir.join("home"))).unwrap();
    std::fs::create_dir_all(dir.join("work")).unwrap();
    Fixture { store, dir }
}

fn shell() -> (PathBuf, Vec<String>) {
    if cfg!(windows) {
        let comspec = std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        (comspec, vec!["/q".to_string()])
    } else {
        (PathBuf::from("/bin/sh"), Vec::new())
    }
}

fn start_terminal(fx: &Fixture, initial_input: Option<&str>) -> (AgentId, RuntimeInfo) {
    let cwd = fx.dir.join("work");
    let record = fx
        .store
        .create_agent(NewAgent {
            session: "default".into(),
            name: "e2e".into(),
            kind: AgentKind::Terminal,
            cwd: cwd.clone(),
            task: initial_input.unwrap_or_default().to_string(),
        })
        .unwrap();
    let (program, args) = shell();
    let spec = LaunchSpec {
        kind: AgentKind::Terminal,
        program,
        args,
        cwd,
        env: vec![("ORKAI_E2E".into(), "1".into())],
        env_remove: Vec::new(),
        initial_input: initial_input.map(str::to_string),
        rows: 24,
        cols: 100,
    };
    fx.store.write_launch(record.id, &spec).unwrap();
    spawn_holder(fx.store.home(), record.id, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let runtime = wait_for_runtime(&fx.store, record.id, TIMEOUT).unwrap();
    (record.id, runtime)
}

fn open(runtime: &RuntimeInfo) -> (Connection, Receiver<Option<HolderMsg>>) {
    let (conn, reader, welcome) = client::connect(runtime).unwrap();
    assert_eq!(welcome.protocol, PROTOCOL_VERSION);
    let (tx, rx) = mpsc::channel();
    client::spawn_reader(reader, move |msg| tx.send(msg).is_ok()).unwrap();
    (conn, rx)
}

fn wait_for_text(rx: &Receiver<Option<HolderMsg>>, parser: &mut vt100::Parser, needle: &str) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if parser.screen().contents().contains(needle) {
            return;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "timed out waiting for {needle:?}; screen:\n{}", parser.screen().contents());
        match rx.recv_timeout(remaining) {
            Ok(Some(HolderMsg::Snapshot(snapshot))) => {
                *parser = vt100::Parser::new(snapshot.size.rows, snapshot.size.cols, 500);
                parser.process(&snapshot.data);
            }
            Ok(Some(HolderMsg::Output(bytes))) => parser.process(&bytes),
            Ok(Some(_)) => {}
            Ok(None) => panic!("holder disconnected while waiting for {needle:?}"),
            Err(_) => {}
        }
    }
}

fn wait_for_exit(rx: &Receiver<Option<HolderMsg>>) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "timed out waiting for exit");
        match rx.recv_timeout(remaining) {
            Ok(Some(HolderMsg::Exited(_))) | Ok(None) => return,
            _ => {}
        }
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "condition not reached in time");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn agent_survives_client_disconnect_and_can_be_reattached() {
    let fx = fixture("survive");
    let (id, runtime) = start_terminal(&fx, Some("echo orkai-first-marker"));

    let (conn, rx) = open(&runtime);
    conn.send(&ClientMsg::Attach(TermSize { rows: 24, cols: 100 })).unwrap();
    let mut screen = vt100::Parser::new(24, 100, 500);
    wait_for_text(&rx, &mut screen, "orkai-first-marker");
    drop(conn);
    drop(rx);

    std::thread::sleep(Duration::from_millis(300));
    let runtime_again = fx.store.read_runtime(id).unwrap().expect("holder must still be running");
    assert_eq!(runtime_again.holder_pid, runtime.holder_pid);

    let (conn, rx) = open(&runtime_again);
    conn.send(&ClientMsg::Attach(TermSize { rows: 30, cols: 110 })).unwrap();
    let mut screen = vt100::Parser::new(30, 110, 500);
    wait_for_text(&rx, &mut screen, "orkai-first-marker");
    assert_eq!(screen.screen().size(), (30, 110));

    conn.send(&ClientMsg::Input(b"echo orkai-second-marker\r".to_vec())).unwrap();
    wait_for_text(&rx, &mut screen, "orkai-second-marker");

    conn.send(&ClientMsg::Input(b"exit\r".to_vec())).unwrap();
    wait_for_exit(&rx);
    wait_until(|| fx.store.read_runtime(id).unwrap().is_none());
    let exit = fx.store.read_exit(id).unwrap().expect("exit must be recorded");
    assert_eq!(exit.code, Some(0));

    let replayed = oplog::replay(&fx.store.home().output_log(id)).unwrap().unwrap();
    let contents = replayed.screen().contents();
    assert!(contents.contains("orkai-second-marker"), "log replay lost output:\n{contents}");
}

#[test]
fn kill_terminates_the_agent_and_records_exit() {
    let fx = fixture("kill");
    let (id, runtime) = start_terminal(&fx, None);
    let (conn, rx) = open(&runtime);
    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
    wait_until(|| fx.store.read_exit(id).unwrap().is_some());
    wait_until(|| fx.store.read_runtime(id).unwrap().is_none());
}

#[test]
fn status_is_pushed_to_every_client() {
    let fx = fixture("status");
    let (_id, runtime) = start_terminal(&fx, None);
    let (conn, rx) = open(&runtime);
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(HolderMsg::Status(status))) => break status,
            Ok(Some(_)) => {}
            other => panic!("no status received: {other:?}"),
        }
    };
    assert!(status.started_at_ms > 0);
    assert_eq!((status.rows, status.cols), (24, 100));
    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
}

#[test]
fn connection_with_wrong_token_is_rejected() {
    let fx = fixture("token");
    let (_id, runtime) = start_terminal(&fx, None);
    let mut forged = runtime.clone();
    forged.token = "0".repeat(64);
    assert!(client::connect(&forged).is_err());

    let mut raw = TcpStream::connect(("127.0.0.1", runtime.port)).unwrap();
    raw.write_all(&ClientMsg::Input(b"echo pwned\r".to_vec()).to_frame()).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    assert!(HolderMsg::read(&mut raw).is_err());

    let (conn, rx) = open(&runtime);
    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
}

#[test]
fn failing_program_records_error() {
    let fx = fixture("fail");
    let cwd = fx.dir.join("work");
    let record = fx
        .store
        .create_agent(NewAgent {
            session: "default".into(),
            name: "broken".into(),
            kind: AgentKind::Terminal,
            cwd: cwd.clone(),
            task: String::new(),
        })
        .unwrap();
    let spec = LaunchSpec {
        kind: AgentKind::Terminal,
        program: fx.dir.join("definitely-missing-program"),
        args: Vec::new(),
        cwd,
        env: Vec::new(),
        env_remove: Vec::new(),
        initial_input: None,
        rows: 24,
        cols: 80,
    };
    fx.store.write_launch(record.id, &spec).unwrap();
    spawn_holder(fx.store.home(), record.id, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let outcome = wait_for_runtime(&fx.store, record.id, TIMEOUT);
    match outcome {
        Err(err) => assert!(err.to_string().contains("failed") || err.to_string().contains("exited"), "{err}"),
        Ok(runtime) => {
            let (_conn, rx) = open(&runtime);
            wait_for_exit(&rx);
            let exit = fx.store.read_exit(record.id).unwrap().unwrap();
            assert_ne!(exit.code, Some(0));
        }
    }
}
