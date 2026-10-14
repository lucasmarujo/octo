use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use orkai::client::{self, Connection};
use orkai::fsutil::write_json_atomic;
use orkai::launch::{spawn_holder, wait_for_runtime};
use orkai::model::{AgentKind, LaunchSpec, NewAgent};
use orkai::paths::Home;
use orkai::protocol::{ClientMsg, HolderMsg, TermSize};
use orkai::store::Store;
use orkai::util::now_ms;

const TIMEOUT: Duration = Duration::from_secs(45);
const ROWS: u16 = 40;
const COLS: u16 = 140;

struct Fixture {
    dir: PathBuf,
    outer: Store,
    inner: Store,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for store in [&self.inner, &self.outer] {
            for record in store.list_agents().unwrap_or_default() {
                if let Ok(Some(runtime)) = store.read_runtime(record.id) {
                    if let Ok((conn, _, _)) = client::connect(&runtime) {
                        let _ = conn.send(&ClientMsg::Kill);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!("orkai-tui-e2e-{}-{}", std::process::id(), now_ms()));
    let outer = Store::open(Home::at(dir.join("outer"))).unwrap();
    let inner = Store::open(Home::at(dir.join("inner"))).unwrap();
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let (shell, args) = if cfg!(windows) { ("cmd.exe", vec!["/q"]) } else { ("/bin/sh", vec![]) };
    write_json_atomic(&inner.home().config_file(), &serde_json::json!({ "shell": shell, "shell_args": args })).unwrap();
    Fixture { dir, outer, inner }
}

struct Session {
    conn: Connection,
    rx: Receiver<Option<HolderMsg>>,
    screen: vt100::Parser,
}

fn start_tui(fx: &Fixture) -> Session {
    let record = fx
        .outer
        .create_agent(NewAgent {
            session: "default".into(),
            name: "tui".into(),
            kind: AgentKind::Terminal,
            cwd: fx.dir.join("work"),
            task: String::new(),
        })
        .unwrap();
    let spec = LaunchSpec {
        kind: AgentKind::Terminal,
        program: PathBuf::from(env!("CARGO_BIN_EXE_orkai")),
        args: Vec::new(),
        cwd: fx.dir.join("work"),
        env: vec![
            ("ORKAI_HOME".into(), fx.inner.home().root().to_string_lossy().into_owned()),
            ("TERM".into(), "xterm-256color".into()),
        ],
        env_remove: Vec::new(),
        initial_input: None,
        rows: ROWS,
        cols: COLS,
    };
    fx.outer.write_launch(record.id, &spec).unwrap();
    spawn_holder(fx.outer.home(), record.id, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let runtime = wait_for_runtime(&fx.outer, record.id, TIMEOUT).unwrap();
    let (conn, reader, _) = client::connect(&runtime).unwrap();
    let (tx, rx) = mpsc::channel();
    client::spawn_reader(reader, move |msg| tx.send(msg).is_ok()).unwrap();
    conn.send(&ClientMsg::Attach(TermSize { rows: ROWS, cols: COLS })).unwrap();
    Session { conn, rx, screen: vt100::Parser::new(ROWS, COLS, 0) }
}

impl Session {
    fn pump(&mut self, wait: Duration) -> bool {
        match self.rx.recv_timeout(wait) {
            Ok(Some(HolderMsg::Snapshot(snapshot))) => {
                self.screen = vt100::Parser::new(snapshot.size.rows, snapshot.size.cols, 0);
                self.screen.process(&snapshot.data);
                true
            }
            Ok(Some(HolderMsg::Output(bytes))) => {
                self.screen.process(&bytes);
                true
            }
            Ok(Some(HolderMsg::Exited(_))) | Ok(None) => false,
            Ok(Some(_)) | Err(_) => true,
        }
    }

    fn wait_for(&mut self, needle: &str) {
        let deadline = Instant::now() + TIMEOUT;
        while !self.screen.screen().contents().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}; screen:\n{}",
                self.screen.screen().contents()
            );
            if !self.pump(Duration::from_millis(100)) {
                panic!("tui exited while waiting for {needle:?}:\n{}", self.screen.screen().contents());
            }
        }
    }

    fn wait_for_exit(&mut self) {
        let deadline = Instant::now() + TIMEOUT;
        while self.pump(Duration::from_millis(100)) {
            assert!(Instant::now() < deadline, "tui did not exit");
        }
    }

    fn send(&self, bytes: &[u8]) {
        self.conn.send(&ClientMsg::Input(bytes.to_vec())).unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }

    fn type_text(&self, text: &str) {
        for c in text.chars() {
            self.conn.send(&ClientMsg::Input(c.to_string().into_bytes())).unwrap();
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn dump(&self, name: &str) {
        let Ok(dir) = std::env::var("ORKAI_RENDER_DUMP_DIR") else { return };
        let screen = self.screen.screen();
        let (rows, cols) = screen.size();
        let color = |c: vt100::Color| match c {
            vt100::Color::Rgb(r, g, b) => format!("\"#{r:02x}{g:02x}{b:02x}\""),
            vt100::Color::Default => "null".to_string(),
            vt100::Color::Idx(i) => format!("\"idx{i}\""),
        };
        let mut cells = Vec::new();
        for row in 0..rows {
            for col in 0..cols {
                let cell = screen.cell(row, col).unwrap();
                let symbol = if cell.is_wide_continuation() { "" } else { cell.contents() };
                let (fg, bg) =
                    if cell.inverse() { (cell.bgcolor(), cell.fgcolor()) } else { (cell.fgcolor(), cell.bgcolor()) };
                cells.push(format!(
                    "[{},{},{},{}]",
                    serde_json::to_string(symbol).unwrap(),
                    color(fg),
                    color(bg),
                    cell.bold()
                ));
            }
        }
        let json = format!("{{\"width\":{cols},\"height\":{rows},\"cells\":[{}]}}", cells.join(","));
        std::fs::write(Path::new(&dir).join(format!("{name}.json")), json).unwrap();
    }
}

#[test]
fn tui_launches_agent_opens_it_and_agent_survives_restart() {
    let fx = fixture();
    let mut tui = start_tui(&fx);
    tui.wait_for("write a task");
    tui.wait_for("who claude");

    tui.send(b"\t");
    tui.send(b"\t");
    tui.wait_for("who terminal");

    tui.type_text("echo orkai-tui-marker");
    tui.send(b"\r");
    tui.wait_for("$ echo-orkai-tui");
    tui.wait_for("echo orkai-tui-marker");
    tui.dump("overview");

    tui.send(b"\r");
    tui.wait_for("ctrl+] overview");
    tui.wait_for("orkai-tui-marker");
    tui.dump("agent");

    tui.send(b"\x1d");
    tui.wait_for("write a task");

    tui.send(b"\x03");
    tui.wait_for_exit();

    let agents = fx.inner.list_agents().unwrap();
    assert_eq!(agents.len(), 1);
    let agent = &agents[0];
    assert_eq!(agent.name, "echo-orkai-tui");
    let runtime = fx.inner.read_runtime(agent.id).unwrap().expect("agent must survive the tui exiting");
    let (conn, _reader, _) = client::connect(&runtime).expect("agent holder must still accept connections");
    drop(conn);

    let mut again = start_tui(&fx);
    again.wait_for("echo-orkai-tui");
    again.wait_for("$ echo-orkai-tui");
    again.dump("restarted");
    again.send(b"\x03");
    again.wait_for_exit();
}
