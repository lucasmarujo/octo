use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use orkai::launch::AGENT_ID_ENV;
use orkai::model::{AgentKind, AgentRecord, NewAgent};
use orkai::paths::{HOME_ENV, Home};
use orkai::store::Store;
use orkai::util::now_ms;
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(60);

struct Fixture {
    dir: PathBuf,
    store: Store,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!("orkai-mcp-e2e-{}-{}", std::process::id(), now_ms()));
    let store = Store::open(Home::at(dir.join("home"))).unwrap();
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let shell = if cfg!(windows) {
        let comspec = std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
        json!({ "shell": comspec, "shell_args": ["/q"] })
    } else {
        json!({ "shell": "/bin/sh", "shell_args": [] })
    };
    std::fs::write(store.home().config_file(), shell.to_string()).unwrap();
    Fixture { dir, store }
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Mcp {
    fn start(fx: &Fixture, lead: &AgentRecord) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_orkai"))
            .arg("mcp")
            .env(HOME_ENV, fx.store.home().root())
            .env(AGENT_ID_ENV, lead.id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self { child, stdin, stdout, next: 1 }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
        let mut reply = String::new();
        self.stdout.read_line(&mut reply).unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let reply = self.request("tools/call", json!({ "name": name, "arguments": arguments }));
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap();
        assert_eq!(result["isError"], false, "{name} failed: {text}");
        serde_json::from_str(text).unwrap()
    }
}

fn screen_text(value: &Value) -> String {
    value["screen"].as_array().unwrap().iter().map(|line| line.as_str().unwrap()).collect::<Vec<_>>().join("\n")
}

#[test]
fn an_agent_starts_follows_answers_and_stops_another_agent_over_mcp() {
    let fx = fixture();
    let mut lead = fx
        .store
        .create_agent(NewAgent {
            session: "team".into(),
            name: "lead".into(),
            kind: AgentKind::Claude,
            cwd: fx.dir.join("work"),
            task: String::new(),
        })
        .unwrap();
    lead.bypass = true;
    fx.store.save_agent(&lead).unwrap();
    let mut mcp = Mcp::start(&fx, &lead);
    let init = mcp.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {} }));
    assert_eq!(init["result"]["serverInfo"]["name"], "orkai");
    writeln!(mcp.stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();

    let started = mcp.tool(
        "start_agent",
        json!({ "agent": "terminal", "task": "echo orkai-mcp-child", "folder": fx.dir.join("work"), "name": "helper" }),
    );
    let id = u32::try_from(started["id"].as_u64().unwrap()).unwrap();
    assert_eq!((started["name"].as_str(), started["session"].as_str()), (Some("helper"), Some("team")));
    assert_eq!(fx.store.load_agent(id).unwrap().parent, Some(lead.id));

    let deadline = Instant::now() + TIMEOUT;
    loop {
        let read = mcp.tool("read_agent", json!({ "id": id, "lines": 20 }));
        if screen_text(&read).lines().any(|line| line.trim() == "orkai-mcp-child") {
            break;
        }
        assert!(Instant::now() < deadline, "the child never ran its task: {read}");
        std::thread::sleep(Duration::from_millis(200));
    }

    let waited = mcp.tool("wait_for_agent", json!({ "id": id, "timeout_seconds": 20 }));
    assert_eq!(waited["still_working"], false, "{waited}");
    let answered = mcp.tool(
        "send_to_agent",
        json!({ "id": id, "text": "echo orkai-mcp-again", "wait": true, "timeout_seconds": 20 }),
    );
    assert!(screen_text(&answered).contains("orkai-mcp-again"), "{answered}");

    let listed = mcp.tool("list_agents", json!({}));
    let helper = listed.as_array().unwrap().iter().find(|agent| agent["id"] == id).unwrap().clone();
    assert_eq!((helper["parent"].as_u64(), helper["kind"].as_str()), (Some(u64::from(lead.id)), Some("terminal")));

    assert_eq!(mcp.tool("stop_agent", json!({ "id": id }))["stopped"], true);
    let ended = mcp.tool("wait_for_agent", json!({ "id": id, "timeout_seconds": 5 }));
    assert_eq!(ended["state"], "exited", "{ended}");
}
