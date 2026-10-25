use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::cli::{self, DEFAULT_SIZE, Probe, Waited};
use crate::holder::nested;
use crate::launch::{AGENT_ID_ENV, Launcher};
use crate::model::{AgentId, AgentKind, AgentRecord, AgentState, DEFAULT_SESSION, NewAgent};
use crate::store::Store;
use crate::tui::format::default_agent_name;
use crate::util::now_ms;

pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
pub const MAX_DEPTH: usize = 3;
pub const MAX_CHILDREN: usize = 8;
const DEFAULT_WAIT_SECS: u64 = 240;
const MAX_WAIT_SECS: u64 = 1_800;
const DEFAULT_LINES: usize = 60;
const MAX_LINES: usize = 400;
const WAIT_LINES: usize = 30;
const PARSE_ERROR: i64 = -32700;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INSTRUCTIONS: &str = "orkai runs coding agents in their own terminals, side by side with the user. \
start_agent hands a task to another agent (claude, codex, opencode…), optionally in its own git worktree; \
wait_for_agent blocks until it finishes its turn, needs an answer or ends; read_agent shows its screen; \
send_to_agent answers it or gives it more work; stop_agent ends it. The agents you start show up under you \
in the user's orkai overview.";

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListArgs {
    all_sessions: bool,
}

#[derive(Deserialize)]
struct StartArgs {
    task: String,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    folder: Option<PathBuf>,
    #[serde(default)]
    worktree: bool,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct SendArgs {
    id: AgentId,
    text: String,
    #[serde(default)]
    wait: bool,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

#[derive(Deserialize)]
struct WaitArgs {
    id: AgentId,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

#[derive(Deserialize)]
struct ReadArgs {
    id: AgentId,
    #[serde(default)]
    lines: Option<usize>,
}

#[derive(Deserialize)]
struct IdArgs {
    id: AgentId,
}

pub struct Server {
    launcher: Launcher,
    caller: Option<AgentRecord>,
}

pub fn run(launcher: Launcher) -> io::Result<()> {
    let caller = find_caller(launcher.store());
    let server = Server::new(launcher, caller);
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

pub fn find_caller(store: &Store) -> Option<AgentRecord> {
    let from_env = std::env::var(AGENT_ID_ENV).ok().and_then(|id| id.parse::<AgentId>().ok());
    if let Some(record) = from_env.and_then(|id| store.load_agent(id).ok()) {
        return Some(record);
    }
    let me = std::process::id();
    store.list_agents().ok()?.into_iter().find(|record| {
        store
            .read_runtime(record.id)
            .ok()
            .flatten()
            .and_then(|runtime| runtime.child_pid)
            .is_some_and(|child| nested::descendants(child).iter().any(|process| process.pid == me))
    })
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn arguments<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|err| format!("invalid arguments: {err}"))
}

fn tools() -> Value {
    let id = json!({ "type": "integer", "description": "The agent's number, as start_agent or list_agents gives it" });
    let timeout = json!({ "type": "integer", "description": "Give up after this many seconds (default 240, at most 1800); call again to keep waiting" });
    json!([
        {
            "name": "list_agents",
            "description": "List the agents in orkai with their state (working, paused = waiting for an answer, done, idle, exited), folder, branch and what they are doing.",
            "inputSchema": { "type": "object", "properties": {
                "all_sessions": { "type": "boolean", "description": "Include every session, not only yours" }
            } }
        },
        {
            "name": "start_agent",
            "description": "Start another agent on a task, in its own terminal. It starts working right away; use wait_for_agent to know when it is done.",
            "inputSchema": { "type": "object", "required": ["task"], "properties": {
                "task": { "type": "string", "description": "What the agent should do, as you would type it to that agent" },
                "agent": { "type": "string", "description": "claude, codex, kilo, opencode, agy, grok, qwen, kimi or terminal (a shell running the task as a command, only for a lead in bypass mode). Defaults to your own kind" },
                "folder": { "type": "string", "description": "Folder to work in, absolute or relative to yours. Defaults to your folder" },
                "worktree": { "type": "boolean", "description": "Work in a new git worktree and branch, so its changes stay apart from yours" },
                "name": { "type": "string", "description": "Short name shown in orkai" }
            } }
        },
        {
            "name": "wait_for_agent",
            "description": "Wait until an agent finishes its turn, needs an answer (paused) or ends, then return its state and the last lines of its screen.",
            "inputSchema": { "type": "object", "required": ["id"], "properties": { "id": id, "timeout_seconds": timeout } }
        },
        {
            "name": "read_agent",
            "description": "Read the last lines of an agent's terminal.",
            "inputSchema": { "type": "object", "required": ["id"], "properties": {
                "id": id,
                "lines": { "type": "integer", "description": "How many lines (default 60, at most 400)" }
            } }
        },
        {
            "name": "send_to_agent",
            "description": "Type a message into an agent you started and press enter, to answer it or give it more work.",
            "inputSchema": { "type": "object", "required": ["id", "text"], "properties": {
                "id": id,
                "text": { "type": "string" },
                "wait": { "type": "boolean", "description": "Also wait until the agent finishes the turn this message starts" },
                "timeout_seconds": timeout
            } }
        },
        {
            "name": "stop_agent",
            "description": "Stop an agent you started. Its record, output and worktree stay in orkai until the user removes it.",
            "inputSchema": { "type": "object", "required": ["id"], "properties": { "id": id } }
        }
    ])
}

fn wait_limit(seconds: Option<u64>) -> Duration {
    Duration::from_secs(seconds.unwrap_or(DEFAULT_WAIT_SECS).clamp(1, MAX_WAIT_SECS))
}

fn tail(lines: Vec<String>, count: usize) -> Vec<String> {
    let start = lines.len().saturating_sub(count);
    lines.into_iter().skip(start).collect()
}

impl Server {
    pub fn new(launcher: Launcher, caller: Option<AgentRecord>) -> Self {
        Self { launcher, caller }
    }

    fn store(&self) -> &Store {
        self.launcher.store()
    }

    pub fn handle_line(&self, line: &str) -> Option<Value> {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(err) => return Some(error(Value::Null, PARSE_ERROR, err.to_string())),
        };
        let id = message.get("id").cloned()?;
        let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        Some(match self.dispatch(method, params) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, text)) => error(id, code, text),
        })
    }

    fn dispatch(&self, method: &str, params: Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
                let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "orkai", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let outcome = match name.as_str() {
                    "list_agents" => arguments(args).and_then(|args| self.list_agents(args)),
                    "start_agent" => arguments(args).and_then(|args| self.start_agent(args)),
                    "wait_for_agent" => arguments(args).and_then(|args| self.wait_for_agent(args)),
                    "read_agent" => arguments(args).and_then(|args| self.read_agent(args)),
                    "send_to_agent" => arguments(args).and_then(|args| self.send_to_agent(args)),
                    "stop_agent" => arguments(args).and_then(|args| self.stop_agent(args)),
                    other => return Err((INVALID_PARAMS, format!("unknown tool {other}"))),
                };
                Ok(match outcome {
                    Ok(value) => json!({
                        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }],
                        "isError": false,
                    }),
                    Err(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": true }),
                })
            }
            other => Err((METHOD_NOT_FOUND, format!("unknown method {other}"))),
        }
    }

    fn list_agents(&self, args: ListArgs) -> Result<Value, String> {
        let session = self.caller.as_ref().filter(|_| !args.all_sessions).map(|caller| caller.session.as_str());
        let agents = cli::listing(self.store(), session).map_err(|err| err.to_string())?;
        serde_json::to_value(agents).map_err(|err| err.to_string())
    }

    fn depth(&self, record: &AgentRecord) -> usize {
        let mut depth = 0;
        let mut current = record.parent;
        while let Some(id) = current {
            depth += 1;
            if depth > MAX_DEPTH {
                break;
            }
            current = self.store().load_agent(id).ok().and_then(|parent| parent.parent);
        }
        depth
    }

    fn running_children(&self, parent: AgentId) -> Result<usize, String> {
        let records = self.store().list_agents().map_err(|err| err.to_string())?;
        Ok(records
            .iter()
            .filter(|record| record.parent == Some(parent))
            .filter(|record| self.store().read_exit(record.id).ok().flatten().is_none())
            .filter(|record| self.store().read_runtime(record.id).ok().flatten().is_some())
            .count())
    }

    fn start_agent(&self, args: StartArgs) -> Result<Value, String> {
        let task = args.task.trim().to_string();
        if task.is_empty() {
            return Err("the task is empty".into());
        }
        let caller = self.caller.as_ref();
        let kind = match args.agent.as_deref() {
            Some(agent) => {
                AgentKind::parse(agent).ok_or_else(|| format!("agent must be one of {}", AgentKind::names()))?
            }
            None => caller.map_or(AgentKind::Claude, |caller| caller.kind),
        };
        if let Some(caller) = caller {
            if !kind.is_ai() && !caller.bypass {
                return Err("only an agent in bypass mode can start terminals; start an AI agent instead".into());
            }
            if self.depth(caller) + 1 > MAX_DEPTH {
                return Err(format!("agents started by agents go at most {MAX_DEPTH} levels deep"));
            }
            if self.running_children(caller.id)? >= MAX_CHILDREN {
                return Err(format!("you already have {MAX_CHILDREN} agents running; stop one first"));
            }
        }
        let base = match caller {
            Some(caller) => caller.cwd.clone(),
            None => std::env::current_dir().map_err(|err| err.to_string())?,
        };
        let folder = match args.folder {
            Some(folder) if folder.is_absolute() => folder,
            Some(folder) => base.join(folder),
            None => base,
        };
        let folder = folder.canonicalize().map_err(|err| format!("{}: {err}", folder.display()))?;
        if !folder.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
        let names: Vec<String> =
            self.store().list_agents().map_err(|err| err.to_string())?.into_iter().map(|record| record.name).collect();
        let name = match args.name {
            Some(name) => name,
            None => default_agent_name(kind, &task, |candidate| names.iter().any(|n| n == candidate)),
        };
        let session = caller.map_or_else(|| DEFAULT_SESSION.to_string(), |caller| caller.session.clone());
        let bypass = kind.is_ai() && caller.is_some_and(|caller| caller.bypass);
        let draft = NewAgent { session, name, kind, cwd: cli::strip_verbatim(folder), task };
        let record = self
            .launcher
            .create_with_parent(draft, DEFAULT_SIZE, args.worktree && kind.is_ai(), bypass, caller.map(|c| c.id))
            .map_err(|err| err.to_string())?;
        Ok(json!({
            "id": record.id,
            "name": record.name,
            "agent": record.kind,
            "session": record.session,
            "folder": record.cwd,
            "branch": record.worktree.as_ref().map(|worktree| worktree.branch.clone()),
            "bypass": record.bypass,
        }))
    }

    fn owned(&self, id: AgentId) -> Result<AgentRecord, String> {
        let record = self.store().load_agent(id).map_err(|_| format!("agent {id} not found"))?;
        match &self.caller {
            Some(caller) if record.parent != Some(caller.id) => {
                Err(format!("agent {id} was not started by you; you can only message or stop your own agents"))
            }
            _ => Ok(record),
        }
    }

    fn report(&self, record: &AgentRecord, waited: Waited) -> Result<Value, String> {
        let (state, activity, still_working) = match waited {
            Waited::Settled(status) => (status.state, status.activity, false),
            Waited::Exited(exit) => (AgentState::Exited, exit.describe(), false),
            Waited::Lost => (AgentState::Lost, "interrupted".to_string(), false),
            Waited::TimedOut(status) => {
                let state = status.as_ref().map_or(AgentState::Working, |status| status.state);
                (state, status.map(|status| status.activity).unwrap_or_default(), true)
            }
        };
        Ok(json!({
            "id": record.id,
            "name": record.name,
            "state": state,
            "still_working": still_working,
            "activity": activity,
            "screen": self.screen(record.id, WAIT_LINES)?,
        }))
    }

    fn screen(&self, id: AgentId, lines: usize) -> Result<Vec<String>, String> {
        let transcript = cli::transcript(self.store(), id).map_err(|err| err.to_string())?;
        Ok(tail(transcript.unwrap_or_default(), lines))
    }

    fn wait_for_agent(&self, args: WaitArgs) -> Result<Value, String> {
        let record = self.store().load_agent(args.id).map_err(|_| format!("agent {} not found", args.id))?;
        let waited = cli::wait_turn(self.store(), record.id, 0, Some(wait_limit(args.timeout_seconds)))
            .map_err(|err| err.to_string())?;
        self.report(&record, waited)
    }

    fn read_agent(&self, args: ReadArgs) -> Result<Value, String> {
        let record = self.store().load_agent(args.id).map_err(|_| format!("agent {} not found", args.id))?;
        let state = cli::probe(self.store(), record.id).state();
        let lines = args.lines.unwrap_or(DEFAULT_LINES).clamp(1, MAX_LINES);
        Ok(json!({ "id": record.id, "name": record.name, "state": state, "screen": self.screen(record.id, lines)? }))
    }

    fn send_to_agent(&self, args: SendArgs) -> Result<Value, String> {
        let record = self.owned(args.id)?;
        if !matches!(cli::probe(self.store(), record.id), Probe::Live(_)) {
            return Err(format!("agent {} is not running", record.id));
        }
        let sent_at = now_ms();
        cli::send(self.store(), record.id, &args.text, true).map_err(|err| err.to_string())?;
        if !args.wait {
            return Ok(json!({ "id": record.id, "sent": true }));
        }
        let waited = cli::wait_turn(self.store(), record.id, sent_at, Some(wait_limit(args.timeout_seconds)))
            .map_err(|err| err.to_string())?;
        self.report(&record, waited)
    }

    fn stop_agent(&self, args: IdArgs) -> Result<Value, String> {
        let record = self.owned(args.id)?;
        cli::stop(self.store(), record.id).map_err(|err| err.to_string())?;
        Ok(json!({ "id": record.id, "stopped": true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::paths::Home;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

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
        let dir = std::env::temp_dir().join(format!(
            "orkai-mcp-{}-{}-{}",
            std::process::id(),
            now_ms(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        Fixture { dir, store }
    }

    fn server(fx: &Fixture, caller: Option<AgentRecord>) -> Server {
        let launcher = Launcher::new(fx.store.clone(), Config::default(), &std::env::current_exe().unwrap()).unwrap();
        Server::new(launcher, caller)
    }

    fn agent(fx: &Fixture, name: &str, parent: Option<AgentId>) -> AgentRecord {
        let draft = NewAgent {
            session: "team".into(),
            name: name.into(),
            kind: AgentKind::Claude,
            cwd: fx.dir.clone(),
            task: String::new(),
        };
        let mut record = fx.store.create_agent(draft).unwrap();
        record.parent = parent;
        fx.store.save_agent(&record).unwrap();
        record
    }

    fn call(server: &Server, line: &str) -> Value {
        server.handle_line(line).unwrap()
    }

    fn tool(server: &Server, name: &str, arguments: Value) -> (bool, String) {
        let request = json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
        let reply = call(server, &request.to_string());
        let result = &reply["result"];
        (result["isError"].as_bool().unwrap(), result["content"][0]["text"].as_str().unwrap().to_string())
    }

    #[test]
    fn the_handshake_agrees_on_a_version_and_lists_the_tools() {
        let fx = fixture();
        let server = server(&fx, None);
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;
        let reply = call(&server, init);
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(reply["result"]["serverInfo"]["name"], "orkai");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        let future = r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#;
        assert_eq!(call(&server, future)["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);

        assert_eq!(server.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
        assert_eq!(call(&server, r#"{"jsonrpc":"2.0","id":"p","method":"ping"}"#)["result"], json!({}));
        let tools = call(&server, r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#);
        let names: Vec<&str> =
            tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            ["list_agents", "start_agent", "wait_for_agent", "read_agent", "send_to_agent", "stop_agent"]
        );
        assert!(tools["result"]["tools"].as_array().unwrap().iter().all(|t| t["inputSchema"]["type"] == "object"));
    }

    #[test]
    fn bad_requests_get_json_rpc_errors() {
        let fx = fixture();
        let server = server(&fx, None);
        assert_eq!(call(&server, "{ nope")["error"]["code"], PARSE_ERROR);
        assert_eq!(
            call(&server, r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#)["error"]["code"],
            METHOD_NOT_FOUND
        );
        let unknown = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"rm_rf","arguments":{}}}"#;
        assert_eq!(call(&server, unknown)["error"]["code"], INVALID_PARAMS);
        let (failed, text) = tool(&server, "read_agent", json!({ "id": "seven" }));
        assert!(failed && text.starts_with("invalid arguments"), "{text}");
        let (failed, text) = tool(&server, "wait_for_agent", json!({ "id": 99 }));
        assert!(failed && text == "agent 99 not found", "{text}");
        let (failed, text) = tool(&server, "start_agent", json!({ "task": "   " }));
        assert!(failed && text == "the task is empty", "{text}");
        let (failed, text) = tool(&server, "start_agent", json!({ "task": "x", "agent": "gpt" }));
        assert!(failed && text.starts_with("agent must be one of"), "{text}");
    }

    #[test]
    fn agents_only_steer_the_agents_they_started_and_only_so_deep() {
        let fx = fixture();
        let lead = agent(&fx, "lead", None);
        let mine = agent(&fx, "mine", Some(lead.id));
        let other = agent(&fx, "other", None);
        let mcp = server(&fx, Some(lead.clone()));

        let (failed, text) = tool(&mcp, "send_to_agent", json!({ "id": other.id, "text": "hi" }));
        assert!(failed && text.contains("was not started by you"), "{text}");
        let (failed, text) = tool(&mcp, "stop_agent", json!({ "id": other.id }));
        assert!(failed && text.contains("was not started by you"), "{text}");
        let (failed, text) = tool(&mcp, "send_to_agent", json!({ "id": mine.id, "text": "hi" }));
        assert!(failed && text == format!("agent {} is not running", mine.id), "{text}");

        let (failed, text) = tool(&mcp, "list_agents", json!({}));
        assert!(!failed);
        let listed: Value = serde_json::from_str(&text).unwrap();
        let names: Vec<&str> = listed.as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["lead", "mine", "other"]);
        assert_eq!(listed[1]["parent"], lead.id);
        assert_eq!(listed[1]["state"], "lost");

        let (failed, text) = tool(&mcp, "start_agent", json!({ "task": "rm -rf /", "agent": "terminal" }));
        assert!(failed && text.starts_with("only an agent in bypass mode can start terminals"), "{text}");

        let deep = agent(&fx, "deep", Some(mine.id));
        let deeper = agent(&fx, "deeper", Some(deep.id));
        let (failed, text) = tool(&server(&fx, Some(deeper)), "start_agent", json!({ "task": "more" }));
        assert!(failed && text.contains("at most 3 levels deep"), "{text}");
    }
}
