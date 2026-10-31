use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use orkai::client;
use orkai::config::Config;
use orkai::launch::Launcher;
use orkai::model::{AgentId, AgentKind, LaunchSpec, NewAgent};
use orkai::paths::Home;
use orkai::protocol::ClientMsg;
use orkai::pwa::bridge::pairing_link;
use orkai::pwa::crypto::{self, Secret};
use orkai::pwa::hub::Hub;
use orkai::pwa::server::{Server, UNPAIRED};
use orkai::store::Store;

const TIMEOUT: Duration = Duration::from_secs(25);
static COUNTER: AtomicU64 = AtomicU64::new(0);

const HARNESS: &str = r#"
const wire = require(process.argv[2]);
const [port, secret, agent] = [process.argv[3], wire.decode(process.argv[4]), Number(process.argv[5])];
const rows = [];
let opened = false;
let typed = false;
const session = new wire.Session(`ws://127.0.0.1:${port}/ws`, secret, {
  message(message) {
    console.log(`MESSAGE ${message.type}`);
    if (message.type === 'state' && !opened && message.agents.some((a) => a.id === agent && !a.starting)) {
      opened = true;
      session.send({ type: 'open', agent, rows: 20, cols: 48 });
    }
    if (message.type !== 'screen') return;
    if (message.full) rows.length = message.rows;
    for (const [row, runs] of message.lines) rows[row] = runs.map((run) => run[0]).join('');
    if (!typed) {
      typed = true;
      console.log(`SIZE ${message.rows}x${message.cols}`);
      session.send({ type: 'send', text: 'echo wire-$((6*7))' });
    }
    if (rows.some((line) => line && line.trim() === process.argv[6])) {
      console.log('DONE');
      process.exit(0);
    }
  },
  close(code) {
    console.log(`CLOSE ${code}`);
    process.exit(3);
  },
  error(err) {
    console.log(`ERROR ${err}`);
  },
});
setTimeout(() => {
  console.log('TIMEOUT');
  process.exit(4);
}, 20000);
"#;

const PASTE_HARNESS: &str = r#"
const wire = require(process.argv[2]);
const secret = wire.secretFrom(wire.linkKey(process.argv[4]));
if (!secret) {
  console.log('REJECTED');
  process.exit(5);
}
new wire.Session(`ws://127.0.0.1:${process.argv[3]}/ws`, secret, {
  message(message) {
    console.log(`MESSAGE ${message.type}`);
    if (message.type === 'welcome') process.exit(0);
  },
  close(code) {
    console.log(`CLOSE ${code}`);
    process.exit(3);
  },
});
setTimeout(() => {
  console.log('TIMEOUT');
  process.exit(4);
}, 20000);
"#;

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
        "orkai-pwa-browser-{}-{}-{}",
        std::process::id(),
        orkai::util::now_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Store::open(Home::at(dir.join("home"))).unwrap();
    let launcher = Launcher::new(store.clone(), Config::default(), Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let secret = crypto::secret().unwrap();
    let server = Server::bind(0, Hub::new(launcher, "browser".into()), secret).unwrap();
    let port = server.port();
    std::thread::spawn(move || server.serve());
    Fixture { dir, secret, port, store }
}

fn start_terminal(fx: &Fixture) -> AgentId {
    let cwd = fx.dir.join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let draft = NewAgent {
        session: "default".into(),
        name: "shell".into(),
        kind: AgentKind::Terminal,
        cwd: cwd.clone(),
        task: String::new(),
    };
    let id = fx.store.create_agent(draft).unwrap().id;
    let (program, args) = if cfg!(windows) {
        (PathBuf::from("cmd.exe"), vec!["/q".to_string()])
    } else {
        (PathBuf::from("/bin/sh"), Vec::new())
    };
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

fn run_app_wire(fx: &Fixture, secret: &Secret, agent: AgentId) -> Option<Output> {
    let node = which::which("node").ok()?;
    let harness = fx.dir.join("harness.js");
    std::fs::write(&harness, HARNESS).unwrap();
    let wire = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pwa/web/wire.js");
    let expected = if cfg!(windows) { "wire-$((6*7))" } else { "wire-42" };
    let output = Command::new(node)
        .arg(&harness)
        .arg(&wire)
        .arg(fx.port.to_string())
        .arg(crypto::encode(secret))
        .arg(agent.to_string())
        .arg(expected)
        .output()
        .unwrap();
    Some(output)
}

fn paste_link(fx: &Fixture, pasted: &str) -> Option<Output> {
    let node = which::which("node").ok()?;
    let harness = fx.dir.join("paste.js");
    std::fs::write(&harness, PASTE_HARNESS).unwrap();
    let wire = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pwa/web/wire.js");
    Some(Command::new(node).arg(&harness).arg(&wire).arg(fx.port.to_string()).arg(pasted).output().unwrap())
}

#[test]
fn a_pasted_pairing_link_pairs_without_the_camera() {
    let fx = fixture();
    let link = pairing_link("https://quiet-river-sound-lamp.trycloudflare.com/", &fx.secret);
    let (head, tail) = link.split_at(link.len() / 2);
    let copied_from_a_wrapped_terminal = format!("  {head}\r\n          {tail}  \n");
    for pasted in [link.clone(), copied_from_a_wrapped_terminal, crypto::encode(&fx.secret)] {
        let Some(output) = paste_link(&fx, &pasted) else {
            eprintln!("node is not installed; the pasted link is not exercised");
            return;
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("MESSAGE welcome"), "{pasted:?}: {stdout}");
        assert_eq!(output.status.code(), Some(0), "{pasted:?}: {stdout}");
    }
}

#[test]
fn a_cut_or_bare_link_is_refused_before_it_reaches_the_bridge() {
    let fx = fixture();
    let link = pairing_link("https://quiet-river-sound-lamp.trycloudflare.com", &fx.secret);
    let cut = link[..link.len() - 6].to_string();
    for pasted in [cut, "https://quiet-river-sound-lamp.trycloudflare.com/".to_string(), String::new()] {
        let Some(output) = paste_link(&fx, &pasted) else { return };
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("REJECTED"), "{pasted:?}: {stdout}");
        assert_eq!(output.status.code(), Some(5), "{pasted:?}: {stdout}");
    }
}

#[test]
fn a_pasted_link_of_another_orkai_is_told_it_is_not_paired() {
    let fx = fixture();
    let other = pairing_link("https://quiet-river-sound-lamp.trycloudflare.com", &crypto::secret().unwrap());
    let Some(output) = paste_link(&fx, &other) else { return };
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("CLOSE {UNPAIRED}")), "{stdout}");
    assert!(!stdout.contains("MESSAGE"), "{stdout}");
}

#[test]
fn the_app_wire_pairs_with_the_bridge_and_drives_an_agent() {
    let fx = fixture();
    let id = start_terminal(&fx);
    let Some(output) = run_app_wire(&fx, &fx.secret, id) else {
        eprintln!("node is not installed; the browser wire is not exercised");
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MESSAGE welcome"), "{stdout}");
    assert!(stdout.contains("MESSAGE state"), "{stdout}");
    assert!(stdout.contains("SIZE 20x48"), "the phone did not get a screen its size: {stdout}");
    assert!(stdout.contains("DONE"), "what the app typed never ran in the agent: {stdout}");
    assert_eq!(output.status.code(), Some(0), "{stdout}");
}

#[test]
fn the_app_wire_with_another_secret_is_told_it_is_not_paired() {
    let fx = fixture();
    let id = start_terminal(&fx);
    let Some(output) = run_app_wire(&fx, &crypto::secret().unwrap(), id) else { return };
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("CLOSE {UNPAIRED}")), "{stdout}");
    assert!(!stdout.contains("MESSAGE"), "a phone without the secret read something: {stdout}");
}
