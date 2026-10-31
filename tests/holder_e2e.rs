use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use orkai::cli::probe;
use orkai::client::{self, Connection};
use orkai::config::Config;
use orkai::holder::detect::DONE_GRACE_MS;
use orkai::holder::nested::{descendants, program_name};
use orkai::holder::oplog;
use orkai::hooks::HOOK_COMMAND;
use orkai::launch::{HOLDER_COMMAND, LaunchMode, Launcher, spawn_holder, wait_for_runtime};
use orkai::model::{
    AgentId, AgentKind, AgentState, LaunchSpec, NestedAgent, NewAgent, RuntimeInfo, StatusReport, TestReport,
    TestState, TestTarget,
};
use orkai::paths::{HOME_ENV, Home};
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
        test: None,
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

fn wait_for_snapshot(rx: &Receiver<Option<HolderMsg>>, size: TermSize) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "no screen of {size:?} arrived");
        match rx.recv_timeout(remaining) {
            Ok(Some(HolderMsg::Snapshot(snapshot))) if snapshot.size == size => return,
            Ok(None) => panic!("holder disconnected while waiting for a screen of {size:?}"),
            _ => {}
        }
    }
}

#[test]
fn the_client_that_used_an_agent_last_sizes_it_and_the_others_get_its_screen() {
    let fx = fixture("sizes");
    let (_id, runtime) = start_terminal(&fx, None);
    let desk_size = TermSize { rows: 40, cols: 120 };
    let phone_size = TermSize { rows: 30, cols: 50 };
    let (desk, desk_rx) = open(&runtime);
    desk.send(&ClientMsg::Attach(desk_size)).unwrap();
    wait_for_snapshot(&desk_rx, desk_size);

    let (phone, phone_rx) = open(&runtime);
    phone.send(&ClientMsg::Attach(phone_size)).unwrap();
    wait_for_snapshot(&phone_rx, phone_size);
    wait_for_snapshot(&desk_rx, phone_size);

    let wider = TermSize { rows: 44, cols: 130 };
    desk.send(&ClientMsg::Resize(wider)).unwrap();
    desk.send(&ClientMsg::Input(b"\r".to_vec())).unwrap();
    wait_for_snapshot(&phone_rx, wider);
    wait_for_snapshot(&desk_rx, wider);

    phone.send(&ClientMsg::Input(b"\r".to_vec())).unwrap();
    wait_for_snapshot(&desk_rx, phone_size);
    drop(phone);
    wait_for_snapshot(&desk_rx, wider);

    desk.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&desk_rx);
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
        test: None,
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

fn wait_for_nested(rx: &Receiver<Option<HolderMsg>>, mut matches: impl FnMut(Option<&NestedAgent>) -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "the holder never reported the expected nested agent");
        match rx.recv_timeout(remaining) {
            Ok(Some(HolderMsg::Status(status))) if matches(status.nested.as_ref()) => return,
            Ok(None) => panic!("holder disconnected while waiting for a nested agent"),
            _ => {}
        }
    }
}

#[test]
fn a_terminal_reports_the_claude_session_started_inside_it() {
    let fx = fixture("nested");
    let claude_home = fx.dir.join("claude-home");
    std::fs::create_dir_all(claude_home.join("sessions")).unwrap();
    let cwd = fx.dir.join("work");
    let (command, program) = if cfg!(windows) { ("ping -n 120 127.0.0.1", "ping") } else { ("sleep 120", "sleep") };
    let record = fx
        .store
        .create_agent(NewAgent {
            session: "default".into(),
            name: "nested".into(),
            kind: AgentKind::Terminal,
            cwd: cwd.clone(),
            task: command.into(),
        })
        .unwrap();
    let (shell, args) = shell();
    let spec = LaunchSpec {
        kind: AgentKind::Terminal,
        program: shell,
        args,
        cwd: cwd.clone(),
        env: Vec::new(),
        env_remove: Vec::new(),
        initial_input: Some(command.into()),
        rows: 24,
        cols: 100,
        test: None,
    };
    fx.store.write_launch(record.id, &spec).unwrap();
    let mut holder = std::process::Command::new(env!("CARGO_BIN_EXE_orkai"))
        .arg(HOLDER_COMMAND)
        .arg(record.id.to_string())
        .env(HOME_ENV, fx.store.home().root())
        .env("CLAUDE_CONFIG_DIR", &claude_home)
        .current_dir(fx.store.home().root())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let runtime = wait_for_runtime(&fx.store, record.id, TIMEOUT).unwrap();
    let (conn, rx) = open(&runtime);
    let shell_pid = runtime.child_pid.expect("the holder must report the shell pid");

    let mut child = None;
    wait_until(|| {
        child = descendants(shell_pid).into_iter().find(|p| program_name(&p.name) == program).map(|p| p.pid);
        child.is_some()
    });
    let pid = child.unwrap();
    let session = "5bd0ccb0-0d09-47ca-9524-6eb740341fa9";
    let file = claude_home.join("sessions").join(format!("{pid}.json"));
    let json = serde_json::json!({"pid": pid, "sessionId": session, "cwd": cwd, "kind": "interactive"});
    std::fs::write(&file, json.to_string()).unwrap();
    wait_for_nested(&rx, |nested| {
        nested.is_some_and(|n| {
            n.kind == AgentKind::Claude && n.pid == pid && n.conversation_id.as_deref() == Some(session)
        })
    });

    std::fs::remove_file(&file).unwrap();
    wait_for_nested(&rx, |nested| nested.is_none());

    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
    let _ = holder.wait();
    if cfg!(unix) {
        let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    } else {
        let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).status();
    }
}

fn run_hook(fx: &Fixture, args: &[&str], stdin: &[u8]) -> std::process::ExitStatus {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_orkai"))
        .arg(HOOK_COMMAND)
        .args(args)
        .env(HOME_ENV, fx.store.home().root())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait().unwrap()
}

fn wait_for_state(rx: &Receiver<Option<HolderMsg>>, expected: AgentState) -> StatusReport {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "the holder never reported {expected:?}");
        match rx.recv_timeout(remaining) {
            Ok(Some(HolderMsg::Status(status))) if status.state == expected => return status,
            Ok(None) => panic!("holder disconnected while waiting for {expected:?}"),
            _ => {}
        }
    }
}

#[test]
fn hook_events_from_the_agent_drive_its_state() {
    let fx = fixture("hooks");
    let cwd = fx.dir.join("work");
    let record = fx
        .store
        .create_agent(NewAgent {
            session: "default".into(),
            name: "hooked".into(),
            kind: AgentKind::Claude,
            cwd: cwd.clone(),
            task: String::new(),
        })
        .unwrap();
    let (program, args) = shell();
    let spec = LaunchSpec {
        kind: AgentKind::Claude,
        program,
        args,
        cwd,
        env: Vec::new(),
        env_remove: Vec::new(),
        initial_input: None,
        rows: 24,
        cols: 100,
        test: None,
    };
    fx.store.write_launch(record.id, &spec).unwrap();
    spawn_holder(fx.store.home(), record.id, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let runtime = wait_for_runtime(&fx.store, record.id, TIMEOUT).unwrap();
    let (conn, rx) = open(&runtime);
    let id = record.id.to_string();

    let prompt = br#"{"session_id":"s","hook_event_name":"UserPromptSubmit","prompt":"private task text"}"#;
    assert!(run_hook(&fx, &["claude", &id], prompt).success());
    let working = wait_for_state(&rx, AgentState::Working);
    assert!(working.turn_started_ms.is_some());

    let permission = br#"{"session_id":"s","hook_event_name":"PermissionRequest","tool_name":"Bash"}"#;
    assert!(run_hook(&fx, &["claude", &id], permission).success());
    wait_for_state(&rx, AgentState::Paused);
    std::thread::sleep(Duration::from_millis(DONE_GRACE_MS + 1_000));
    assert_eq!(probe(&fx.store, record.id).state(), AgentState::Paused, "a pending permission must stay paused");

    let stop = br#"{"session_id":"s","hook_event_name":"Stop","stop_hook_active":false}"#;
    assert!(run_hook(&fx, &["claude", &id], stop).success());
    let done = wait_for_state(&rx, AgentState::Done);
    assert!(done.last_turn_ms.is_some());

    assert!(run_hook(&fx, &["claude", &id], prompt).success());
    wait_for_state(&rx, AgentState::Working);
    let turn = r#"{"type":"agent-turn-complete","thread-id":"t","turn-id":"1","input-messages":["private"]}"#;
    assert!(run_hook(&fx, &["codex", &id, turn], b"").success());
    wait_for_state(&rx, AgentState::Done);

    wait_until(|| std::fs::read_dir(fx.store.home().hooks_dir(record.id)).unwrap().next().is_none());
    assert!(run_hook(&fx, &["claude", "999"], stop).success(), "a hook must never fail the agent's tool call");
    assert!(!fx.store.home().agent_dir(999).exists());
    assert!(run_hook(&fx, &["claude", &id], b"not json").success());

    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
}

fn wait_for_test(rx: &Receiver<Option<HolderMsg>>, expected: TestState) -> TestReport {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "the holder never reported a {expected:?} test run");
        match rx.recv_timeout(remaining) {
            Ok(Some(HolderMsg::Status(StatusReport { test: Some(test), .. }))) if test.state == expected => {
                return *test;
            }
            Ok(None) => panic!("holder disconnected while waiting for a {expected:?} test run"),
            _ => {}
        }
    }
}

fn trust_with_cli(fx: &Fixture, repo: &Path) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_orkai"))
        .args(["trust", "-C"])
        .arg(repo)
        .env(HOME_ENV, fx.store.home().root())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_trusted_test_command_runs_each_time_an_agent_in_a_worktree_finishes_a_turn() {
    let fx = fixture("gate");
    let repo = fx.dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    let repo = orkai::worktree::toplevel(&repo).unwrap();
    std::fs::write(repo.join(orkai::project::FILE), r#"{"test":"echo checking && exit 3"}"#).unwrap();
    let said = trust_with_cli(&fx, &repo);
    assert!(said.contains("trusted") && said.contains("echo checking && exit 3"), "{said}");

    let cwd = fx.dir.join("work");
    let record = fx
        .store
        .create_agent(NewAgent {
            session: "default".into(),
            name: "gated".into(),
            kind: AgentKind::Claude,
            cwd: cwd.clone(),
            task: String::new(),
        })
        .unwrap();
    let (program, args) = shell();
    let spec = LaunchSpec {
        kind: AgentKind::Claude,
        program,
        args,
        cwd: cwd.clone(),
        env: Vec::new(),
        env_remove: Vec::new(),
        initial_input: None,
        rows: 24,
        cols: 100,
        test: Some(TestTarget { repo: repo.clone(), dir: cwd }),
    };
    fx.store.write_launch(record.id, &spec).unwrap();
    spawn_holder(fx.store.home(), record.id, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let runtime = wait_for_runtime(&fx.store, record.id, TIMEOUT).unwrap();
    let (conn, rx) = open(&runtime);
    let id = record.id.to_string();
    let prompt = br#"{"session_id":"s","hook_event_name":"UserPromptSubmit","prompt":"x"}"#;
    let stop = br#"{"session_id":"s","hook_event_name":"Stop","stop_hook_active":false}"#;
    let turn = |expected: TestState| {
        assert!(run_hook(&fx, &["claude", &id], prompt).success());
        wait_for_state(&rx, AgentState::Working);
        assert!(run_hook(&fx, &["claude", &id], stop).success());
        wait_for_test(&rx, expected)
    };

    let failed = turn(TestState::Failed);
    assert_eq!((failed.code, failed.command.as_str()), (Some(3), "echo checking && exit 3"));
    let log = std::fs::read_to_string(fx.store.home().test_log(record.id)).unwrap();
    assert!(log.contains("checking"), "{log}");

    std::fs::write(repo.join(orkai::project::FILE), r#"{"test":"exit 0"}"#).unwrap();
    assert_eq!(turn(TestState::Untrusted).command, "exit 0", "an edited file must be trusted again");
    trust_with_cli(&fx, &repo);
    assert_eq!(turn(TestState::Passed).code, Some(0));

    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
}

fn orkai(fx: &Fixture, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_orkai"))
        .args(args)
        .env(HOME_ENV, fx.store.home().root())
        .output()
        .unwrap()
}

#[test]
fn orkai_wait_returns_when_the_turn_ends_and_ls_json_describes_the_agent() {
    let fx = fixture("wait");
    let (program, args) = shell();
    let (id, runtime) = start_ai(&fx, AgentKind::Claude, program, args);
    let (conn, rx) = open(&runtime);
    let agent = id.to_string();
    let prompt = br#"{"session_id":"s","hook_event_name":"UserPromptSubmit","prompt":"x"}"#;
    assert!(run_hook(&fx, &["claude", &agent], prompt).success());
    wait_for_state(&rx, AgentState::Working);

    let impatient = orkai(&fx, &["wait", &agent, "--timeout", "1"]);
    assert!(!impatient.status.success());
    let said = String::from_utf8_lossy(&impatient.stderr);
    assert!(said.contains(&format!("agent {id} is still working after 1s")), "{said}");

    let waiter = std::process::Command::new(env!("CARGO_BIN_EXE_orkai"))
        .args(["wait", &agent, "--timeout", "30"])
        .env(HOME_ENV, fx.store.home().root())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let stop = br#"{"session_id":"s","hook_event_name":"Stop","stop_hook_active":false}"#;
    assert!(run_hook(&fx, &["claude", &agent], stop).success());
    let waited = waiter.wait_with_output().unwrap();
    assert!(waited.status.success());
    let said = String::from_utf8_lossy(&waited.stdout);
    assert!(said.starts_with(&format!("agent {id} done")), "{said}");

    let listed = orkai(&fx, &["ls", "--json"]);
    assert!(listed.status.success());
    let agents: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let entry = agents.as_array().unwrap().iter().find(|a| a["id"] == id).unwrap();
    assert_eq!((entry["state"].as_str(), entry["kind"].as_str()), (Some("done"), Some("claude")));
    assert!(entry["parent"].is_null() && entry["branch"].is_null() && entry["cwd"].is_string(), "{entry}");

    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
    let ended = orkai(&fx, &["wait", &agent]);
    assert!(String::from_utf8_lossy(&ended.stdout).starts_with(&format!("agent {id} exited")));
}

fn start_ai(fx: &Fixture, kind: AgentKind, program: PathBuf, args: Vec<String>) -> (AgentId, RuntimeInfo) {
    let cwd = fx.dir.join("work");
    let record = fx
        .store
        .create_agent(NewAgent {
            session: "default".into(),
            name: kind.label().into(),
            kind,
            cwd: cwd.clone(),
            task: String::new(),
        })
        .unwrap();
    let spec = LaunchSpec {
        kind,
        program,
        args,
        cwd,
        env: Vec::new(),
        env_remove: Vec::new(),
        initial_input: None,
        rows: 24,
        cols: 100,
        test: None,
    };
    fx.store.write_launch(record.id, &spec).unwrap();
    spawn_holder(fx.store.home(), record.id, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    (record.id, wait_for_runtime(&fx.store, record.id, TIMEOUT).unwrap())
}

#[test]
fn plugin_and_qwen_hook_events_drive_the_state_of_the_new_agents() {
    let fx = fixture("plugin-hooks");
    let (program, args) = shell();
    let (id, runtime) = start_ai(&fx, AgentKind::Opencode, program.clone(), args.clone());
    let (conn, rx) = open(&runtime);
    let agent = id.to_string();
    assert!(run_hook(&fx, &["plugin", &agent, "busy"], b"").success());
    let working = wait_for_state(&rx, AgentState::Working);
    assert!(working.turn_started_ms.is_some());
    assert!(run_hook(&fx, &["plugin", &agent, "waiting"], b"").success());
    wait_for_state(&rx, AgentState::Paused);
    assert!(run_hook(&fx, &["plugin", &agent, "busy"], b"").success());
    wait_for_state(&rx, AgentState::Working);
    assert!(run_hook(&fx, &["plugin", &agent, "finished"], b"").success());
    assert!(wait_for_state(&rx, AgentState::Done).last_turn_ms.is_some());
    assert!(run_hook(&fx, &["plugin", &agent, "nonsense"], b"").success());
    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);

    let (id, runtime) = start_ai(&fx, AgentKind::Qwen, program, args);
    let (conn, rx) = open(&runtime);
    let agent = id.to_string();
    let prompt = br#"{"session_id":"s","hook_event_name":"UserPromptSubmit","prompt":"secret"}"#;
    assert!(run_hook(&fx, &["qwen", &agent], prompt).success());
    wait_for_state(&rx, AgentState::Working);
    let permission = br#"{"session_id":"s","hook_event_name":"PermissionRequest","tool_name":"run_shell_command"}"#;
    assert!(run_hook(&fx, &["qwen", &agent], permission).success());
    wait_for_state(&rx, AgentState::Paused);
    let stop = br#"{"session_id":"s","hook_event_name":"Stop","stop_hook_active":false}"#;
    assert!(run_hook(&fx, &["qwen", &agent], stop).success());
    wait_for_state(&rx, AgentState::Done);
    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
}

#[test]
fn antigravity_program_status_reports_drive_its_state() {
    let fx = fixture("osc7501");
    let (program, args) = if cfg!(windows) {
        let script = "$e=[char]27; $b=[char]7; [Console]::Write(\"$e]7501;state=working:app=agy$b\"); Start-Sleep -Milliseconds 1500; [Console]::Write(\"$e]7501;state=blocked:kind=permission$b\"); Start-Sleep -Milliseconds 1500; [Console]::Write(\"$e]7501;state=done$b\"); Start-Sleep -Seconds 60";
        (
            PathBuf::from("powershell.exe"),
            vec!["-NoLogo".to_string(), "-NoProfile".to_string(), "-Command".to_string(), script.to_string()],
        )
    } else {
        let script = r"printf '\033]7501;state=working:app=agy\007'; sleep 1.5; printf '\033]7501;state=blocked:kind=permission\007'; sleep 1.5; printf '\033]7501;state=done\007'; sleep 60";
        (PathBuf::from("/bin/sh"), vec!["-c".to_string(), script.to_string()])
    };
    let (_, runtime) = start_ai(&fx, AgentKind::Antigravity, program, args);
    let (conn, rx) = open(&runtime);
    conn.send(&ClientMsg::Attach(TermSize { rows: 24, cols: 100 })).unwrap();
    wait_for_state(&rx, AgentState::Working);
    wait_for_state(&rx, AgentState::Paused);
    let done = wait_for_state(&rx, AgentState::Done);
    assert!(done.last_turn_ms.is_some());
    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn args_echoing_program(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        let script = dir.join("claude.cmd");
        std::fs::write(&script, "@echo off\r\necho orkai-args: %*\r\nset /p orkai_wait=\r\n").unwrap();
        script
    } else {
        let script = dir.join("claude");
        std::fs::write(&script, "#!/bin/sh\necho \"orkai-args: $*\"\nread orkai_wait\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        script
    }
}

#[test]
fn an_agent_started_in_bypass_mode_gets_the_flag_and_keeps_it_when_recovered() {
    let fx = fixture("bypass");
    let program = args_echoing_program(&fx.dir);
    let config = Config { claude_command: program.to_string_lossy().into_owned(), ..Config::default() };
    let launcher = Launcher::new(fx.store.clone(), config, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let size = TermSize { rows: 24, cols: 200 };
    let draft = NewAgent {
        session: "default".into(),
        name: "bypass".into(),
        kind: AgentKind::Claude,
        cwd: fx.dir.join("work"),
        task: String::new(),
    };
    let mut record = launcher.create(draft, size, false, true).unwrap();
    assert!(record.bypass);
    assert!(fx.store.load_agent(record.id).unwrap().bypass, "the mode is saved with the agent");
    stop_after_seeing(&fx, record.id, size, "--dangerously-skip-permissions");

    launcher.start(&mut record, LaunchMode::Resume, size).unwrap();
    stop_after_seeing(&fx, record.id, size, "--dangerously-skip-permissions");
}

fn stop_after_seeing(fx: &Fixture, id: AgentId, size: TermSize, needle: &str) {
    let runtime = wait_for_runtime(&fx.store, id, TIMEOUT).unwrap();
    let (conn, rx) = open(&runtime);
    conn.send(&ClientMsg::Attach(size)).unwrap();
    let mut screen = vt100::Parser::new(size.rows, size.cols, 500);
    wait_for_text(&rx, &mut screen, needle);
    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
    wait_until(|| fx.store.read_runtime(id).unwrap().is_none());
}

#[test]
fn agents_of_a_race_each_get_their_own_worktree_and_share_the_race() {
    let fx = fixture("race");
    let repo = fx.dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "orkai@example.com"]);
    git(&repo, &["config", "user.name", "orkai"]);
    std::fs::write(repo.join("readme.txt"), "hello\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    let (program, args) = shell();
    let config = Config { shell: Some(program.to_string_lossy().into_owned()), shell_args: args, ..Config::default() };
    let launcher = Launcher::new(fx.store.clone(), config, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let racer = |name: &str| NewAgent {
        session: "default".into(),
        name: name.into(),
        kind: AgentKind::Terminal,
        cwd: repo.clone(),
        task: String::new(),
    };
    let size = TermSize { rows: 24, cols: 100 };
    let first = launcher.create_in_race(racer("first"), size, false, 42).unwrap();
    let second = launcher.create_in_race(racer("second"), size, false, 42).unwrap();
    for record in [&first, &second] {
        let saved = fx.store.load_agent(record.id).unwrap();
        assert_eq!(saved.race, Some(42));
        assert!(saved.worktree.is_some(), "every racer works in its own worktree");
    }
    assert_ne!(first.worktree.as_ref().unwrap().path, second.worktree.as_ref().unwrap().path);

    for record in [first, second] {
        let runtime = wait_for_runtime(&fx.store, record.id, TIMEOUT).unwrap();
        let (conn, rx) = open(&runtime);
        conn.send(&ClientMsg::Kill).unwrap();
        wait_for_exit(&rx);
        wait_until(|| fx.store.read_runtime(record.id).unwrap().is_none());
        orkai::cli::remove(&fx.store, record.id).unwrap();
    }
    assert!(git(&repo, &["branch", "--list", "orkai/*"]).is_empty());
}

#[test]
fn an_agent_started_in_a_worktree_works_on_its_own_branch_until_removed() {
    let fx = fixture("worktree");
    let repo = fx.dir.join("work");
    std::fs::create_dir_all(repo.join("app")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "orkai@example.com"]);
    git(&repo, &["config", "user.name", "orkai"]);
    std::fs::write(repo.join("app").join("main.txt"), "v1\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    let (program, args) = shell();
    let config = Config { shell: Some(program.to_string_lossy().into_owned()), shell_args: args, ..Config::default() };
    let launcher = Launcher::new(fx.store.clone(), config, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let draft = NewAgent {
        session: "default".into(),
        name: "Branch Check".into(),
        kind: AgentKind::Terminal,
        cwd: repo.join("app"),
        task: "git branch --show-current".into(),
    };
    let record = launcher.create(draft, TermSize { rows: 24, cols: 100 }, true, false).unwrap();
    let worktree = record.worktree.clone().expect("the agent must get a worktree");
    assert_eq!(worktree.branch, "orkai/branch-check");
    assert_eq!(record.cwd, worktree.path.join("app"));
    assert_eq!(fx.store.load_agent(record.id).unwrap(), record);
    assert_eq!(record.project_dir(), repo.join("app"));

    let runtime = wait_for_runtime(&fx.store, record.id, TIMEOUT).unwrap();
    let (conn, rx) = open(&runtime);
    conn.send(&ClientMsg::Attach(TermSize { rows: 24, cols: 100 })).unwrap();
    let mut screen = vt100::Parser::new(24, 100, 500);
    wait_for_text(&rx, &mut screen, "orkai/branch-check");
    assert_eq!(git(&repo, &["branch", "--show-current"]), "main");

    let inside = NewAgent {
        session: "default".into(),
        name: "Inside".into(),
        kind: AgentKind::Terminal,
        cwd: record.cwd.clone(),
        task: String::new(),
    };
    let guest = launcher.create(inside, TermSize { rows: 24, cols: 100 }, false, false).unwrap();
    assert_eq!(guest.worktree, None);
    assert_eq!(guest.project, Some(repo.join("app")), "an agent inside the worktree belongs to its project");
    assert_eq!(fx.store.load_agent(guest.id).unwrap().project_dir(), repo.join("app"));
    let guest_runtime = wait_for_runtime(&fx.store, guest.id, TIMEOUT).unwrap();
    let (guest_conn, guest_rx) = open(&guest_runtime);
    guest_conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&guest_rx);
    wait_until(|| fx.store.read_runtime(guest.id).unwrap().is_none());
    orkai::cli::remove(&fx.store, guest.id).unwrap();

    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
    wait_until(|| fx.store.read_runtime(record.id).unwrap().is_none());
    orkai::cli::remove(&fx.store, record.id).unwrap();
    assert!(!worktree.path.exists());
    assert!(git(&repo, &["branch", "--list", "orkai/*"]).is_empty());
    assert!(fx.store.list_agents().unwrap().is_empty());
}

#[test]
fn a_peek_shows_the_screen_without_attaching_or_resizing_the_agent() {
    let fx = fixture("peek");
    let (_id, runtime) = start_terminal(&fx, Some("echo orkai-peek-marker"));
    let deadline = Instant::now() + TIMEOUT;
    let snapshot = loop {
        let snapshot = client::peek(&runtime).unwrap();
        let mut parser = vt100::Parser::new(snapshot.size.rows, snapshot.size.cols, 0);
        parser.process(&snapshot.data);
        if parser.screen().contents().contains("orkai-peek-marker") {
            break snapshot;
        }
        assert!(Instant::now() < deadline, "the peek never showed the marker:\n{}", parser.screen().contents());
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!((snapshot.size.rows, snapshot.size.cols), (24, 100), "a peek never resizes the agent");
    let (conn, rx) = open(&runtime);
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(HolderMsg::Status(status))) => break status,
            Ok(Some(_)) => {}
            other => panic!("no status received: {other:?}"),
        }
    };
    assert_eq!(status.viewers, Some(0), "a peek is not a viewer");
    conn.send(&ClientMsg::Kill).unwrap();
    wait_for_exit(&rx);
}
