use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::fsutil;
use crate::launch::{find_codex_session, is_safe_session_id, recent_codex_rollouts};
use crate::model::{AgentId, AgentKind, AgentRecord, AgentState, NestedAgent};
use crate::util::is_uuid;

pub const SUBAGENT_STALE_MS: u64 = 10 * 60 * 1_000;
pub const MAX_SUBAGENTS: usize = 16;
pub const ACTIVITY_MAX_CHARS: usize = 120;
const TAIL_BYTES: u64 = 64 * 1024;
const MAX_CACHE_ENTRIES: usize = 4_096;
const CLAUDE_TRANSCRIPT_PREFIX: &str = "agent-";
const CLAUDE_TRANSCRIPT_SUFFIX: &str = ".jsonl";
const CLAUDE_META_SUFFIX: &str = ".meta.json";
const CLAUDE_BACKGROUND_SHAPE: &str = "background";
const CLAUDE_HANDBACK_TOOL: &str = "SubagentHandback";
const CLAUDE_STOP_HOOK: &str = "SubagentStop";
const CLAUDE_END_TURN: &str = "end_turn";
const CLAUDE_DEFAULT_ROLE: &str = "subagent";
const CODEX_DEFAULT_ROLE: &str = "subagent";
const CODEX_STARTED_EVENTS: &[&str] = &["task_started"];
const CODEX_FINISHED_EVENTS: &[&str] = &["task_complete", "turn_aborted", "shutdown_complete"];
const SUMMARY_FIELDS: &[&str] =
    &["description", "command", "cmd", "file_path", "pattern", "query", "url", "path", "prompt"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subagent {
    pub id: String,
    pub role: String,
    pub description: String,
    pub activity: String,
    pub started_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parent {
    pub id: AgentId,
    pub kind: AgentKind,
    pub conversation_id: Option<String>,
    pub cwd: PathBuf,
    pub created_at_ms: u64,
    pub busy: bool,
}

pub fn parent_of(
    record: &AgentRecord,
    kind: AgentKind,
    nested: Option<&NestedAgent>,
    state: AgentState,
) -> Option<Parent> {
    if !kind.is_ai() {
        return None;
    }
    Some(Parent {
        id: record.id,
        kind,
        conversation_id: nested.map_or(record.conversation_id.clone(), |n| n.conversation_id.clone()),
        cwd: nested.and_then(|n| n.cwd.clone()).unwrap_or_else(|| record.cwd.clone()),
        created_at_ms: nested.map_or(record.created_at_ms, |n| n.since_ms),
        busy: matches!(state, AgentState::Working | AgentState::Paused),
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Tail {
    finished: bool,
    activity: String,
}

struct CachedTail {
    len: u64,
    modified: u64,
    tail: Tail,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
struct ClaudeMeta {
    agent_type: Option<String>,
    description: Option<String>,
    request_shape: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeRecord {
    #[serde(rename = "type")]
    kind: String,
    message: Option<ClaudeMessage>,
    attachment: Option<ClaudeAttachment>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeMessage {
    stop_reason: Option<String>,
    content: Value,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClaudeAttachment {
    hook_event: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    input: Value,
    text: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexLine {
    #[serde(rename = "type")]
    kind: String,
    payload: Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexSessionMeta {
    id: String,
    source: Value,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct CodexSpawn {
    parent_thread_id: String,
    agent_nickname: Option<String>,
    agent_role: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexPayload {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    arguments: Option<String>,
    message: Option<String>,
}

#[derive(Default)]
pub struct SubagentWatcher {
    claude_home: Option<PathBuf>,
    codex_home: Option<PathBuf>,
    claude_dirs: HashMap<String, PathBuf>,
    claude_metas: HashMap<PathBuf, ClaudeMeta>,
    codex_threads: HashMap<(AgentId, u64), String>,
    codex_spawns: HashMap<PathBuf, Option<CodexSpawn>>,
    tails: HashMap<PathBuf, CachedTail>,
}

impl SubagentWatcher {
    pub fn new(claude_home: Option<PathBuf>, codex_home: Option<PathBuf>) -> Self {
        Self { claude_home, codex_home, ..Self::default() }
    }

    pub fn scan(&mut self, parents: &[Parent], now: u64) -> BTreeMap<AgentId, Vec<Subagent>> {
        let mut found = BTreeMap::new();
        for parent in parents {
            let mut subagents = match parent.kind {
                AgentKind::Claude => self.claude(parent, now),
                AgentKind::Codex => self.codex(parent, now),
                AgentKind::Kilo
                | AgentKind::Opencode
                | AgentKind::Antigravity
                | AgentKind::Grok
                | AgentKind::Qwen
                | AgentKind::Kimi
                | AgentKind::Pi
                | AgentKind::Terminal => Vec::new(),
            };
            if subagents.is_empty() {
                continue;
            }
            subagents.sort_by(|a, b| a.started_at_ms.cmp(&b.started_at_ms).then_with(|| a.id.cmp(&b.id)));
            subagents.truncate(MAX_SUBAGENTS);
            found.insert(parent.id, subagents);
        }
        self.tails.retain(|_, cached| now.saturating_sub(cached.modified) <= SUBAGENT_STALE_MS);
        if self.claude_metas.len() > MAX_CACHE_ENTRIES {
            self.claude_metas.clear();
        }
        if self.codex_spawns.len() > MAX_CACHE_ENTRIES {
            self.codex_spawns.clear();
        }
        if self.codex_threads.len() > MAX_CACHE_ENTRIES {
            self.codex_threads.clear();
        }
        found
    }

    fn claude_dir(&mut self, session_id: &str, cwd: &Path) -> Option<PathBuf> {
        if let Some(dir) = self.claude_dirs.get(session_id) {
            return Some(dir.clone());
        }
        let projects = self.claude_home.as_ref()?.join("projects");
        let project = projects.join(claude_project_slug(cwd));
        let session = if project.is_dir() {
            project.join(session_id)
        } else {
            let transcript = format!("{session_id}{CLAUDE_TRANSCRIPT_SUFFIX}");
            fs::read_dir(&projects)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.join(&transcript).is_file())?
                .join(session_id)
        };
        let dir = session.join("subagents");
        self.claude_dirs.insert(session_id.to_string(), dir.clone());
        Some(dir)
    }

    fn claude(&mut self, parent: &Parent, now: u64) -> Vec<Subagent> {
        let Some(session_id) = parent.conversation_id.as_deref().filter(|id| is_uuid(id)) else { return Vec::new() };
        let Some(dir) = self.claude_dir(session_id, &parent.cwd) else { return Vec::new() };
        let Ok(entries) = fs::read_dir(&dir) else { return Vec::new() };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            let Some(id) =
                name.strip_prefix(CLAUDE_TRANSCRIPT_PREFIX).and_then(|n| n.strip_suffix(CLAUDE_TRANSCRIPT_SUFFIX))
            else {
                continue;
            };
            let Some((len, modified, created)) = stamp(&path) else { continue };
            if now.saturating_sub(modified) > SUBAGENT_STALE_MS {
                continue;
            }
            let meta_path = dir.join(format!("{CLAUDE_TRANSCRIPT_PREFIX}{id}{CLAUDE_META_SUFFIX}"));
            let meta = self.claude_metas.entry(meta_path).or_insert_with_key(|p| read_json(p)).clone();
            if !parent.busy && meta.request_shape.as_deref() != Some(CLAUDE_BACKGROUND_SHAPE) {
                continue;
            }
            let tail = self.tail(&path, len, modified, claude_tail);
            if tail.finished {
                continue;
            }
            found.push(Subagent {
                id: id.to_string(),
                role: meta.agent_type.unwrap_or_else(|| CLAUDE_DEFAULT_ROLE.to_string()),
                description: single_line(&meta.description.unwrap_or_default()),
                activity: tail.activity,
                started_at_ms: created,
            });
        }
        found
    }

    fn codex_thread(&mut self, parent: &Parent, codex_home: &Path) -> Option<String> {
        let key = (parent.id, parent.created_at_ms);
        if let Some(id) = parent.conversation_id.clone().or_else(|| self.codex_threads.get(&key).cloned()) {
            return Some(id);
        }
        let earlier: Vec<String> = self
            .codex_threads
            .iter()
            .filter(|((id, _), _)| *id == parent.id)
            .map(|(_, thread)| thread.clone())
            .collect();
        let id = find_codex_session(codex_home, &parent.cwd, parent.created_at_ms, &earlier)?;
        self.codex_threads.insert(key, id.clone());
        Some(id)
    }

    fn codex(&mut self, parent: &Parent, now: u64) -> Vec<Subagent> {
        let Some(codex_home) = self.codex_home.clone() else { return Vec::new() };
        let Some(thread) = self.codex_thread(parent, &codex_home).filter(|id| is_safe_session_id(id)) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        let since = now.saturating_sub(SUBAGENT_STALE_MS).max(parent.created_at_ms);
        for path in recent_codex_rollouts(&codex_home, since) {
            let spawn = self.codex_spawns.entry(path.clone()).or_insert_with_key(|p| codex_spawn(p)).clone();
            let Some(spawn) = spawn.filter(|s| s.parent_thread_id == thread) else { continue };
            let Some((len, modified, created)) = stamp(&path) else { continue };
            let tail = self.tail(&path, len, modified, codex_tail);
            if tail.finished {
                continue;
            }
            let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            found.push(Subagent {
                id,
                role: spawn.agent_role.unwrap_or_else(|| CODEX_DEFAULT_ROLE.to_string()),
                description: spawn.agent_nickname.unwrap_or_default(),
                activity: tail.activity,
                started_at_ms: created,
            });
        }
        found
    }

    fn tail(&mut self, path: &Path, len: u64, modified: u64, parse: fn(&str) -> Tail) -> Tail {
        if let Some(cached) = self.tails.get(path).filter(|c| c.len == len && c.modified == modified) {
            return cached.tail.clone();
        }
        let mut tail = read_tail(path, len).map(|text| parse(&text)).unwrap_or_default();
        if tail.activity.is_empty()
            && let Some(previous) = self.tails.get(path)
        {
            tail.activity = previous.tail.activity.clone();
        }
        self.tails.insert(path.to_path_buf(), CachedTail { len, modified, tail: tail.clone() });
        tail
    }
}

pub fn claude_project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fsutil::read_json_opt(path).ok().flatten().unwrap_or_default()
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)).unwrap_or(0)
}

fn stamp(path: &Path) -> Option<(u64, u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let modified = millis(meta.modified().ok()?);
    let created = meta.created().map(millis).unwrap_or(modified);
    Some((meta.len(), modified, created))
}

fn read_tail(path: &Path, len: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Some(if start > 0 { text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default() } else { text })
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(ACTIVITY_MAX_CHARS).collect()
}

fn field_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => Some(items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")),
        _ => None,
    }
}

fn summarize(tool: &str, input: &Value) -> String {
    let detail =
        SUMMARY_FIELDS.iter().find_map(|field| input.get(*field).and_then(field_text)).map(|d| single_line(&d));
    match detail {
        Some(detail) if !detail.is_empty() => single_line(&format!("{tool} · {detail}")),
        _ => tool.to_string(),
    }
}

fn claude_tail(text: &str) -> Tail {
    let mut tail = Tail::default();
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<ClaudeRecord>(line) else { continue };
        if record.attachment.and_then(|a| a.hook_event).as_deref() == Some(CLAUDE_STOP_HOOK) {
            tail.finished = true;
        }
        let Some(message) = record.message.filter(|_| record.kind == "assistant") else { continue };
        let blocks: Vec<ContentBlock> = match message.content {
            Value::Array(items) => items.into_iter().filter_map(|item| serde_json::from_value(item).ok()).collect(),
            _ => Vec::new(),
        };
        for block in blocks {
            match (block.kind.as_str(), block.name, block.text) {
                ("tool_use", Some(name), _) if name == CLAUDE_HANDBACK_TOOL => tail.finished = true,
                ("tool_use", Some(name), _) => {
                    tail.finished = false;
                    tail.activity = summarize(&name, &block.input);
                }
                ("text", _, Some(text)) if !text.trim().is_empty() => {
                    tail.activity = single_line(text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default());
                }
                _ => {}
            }
        }
        if message.stop_reason.as_deref() == Some(CLAUDE_END_TURN) {
            tail.finished = true;
        }
    }
    tail
}

fn codex_spawn(path: &Path) -> Option<CodexSpawn> {
    let file = File::open(path).ok()?;
    let first = BufReader::new(file).lines().next()?.ok()?;
    let line: CodexLine = serde_json::from_str(&first).ok()?;
    if line.kind != "session_meta" {
        return None;
    }
    let meta: CodexSessionMeta = serde_json::from_value(line.payload).ok()?;
    let spawn = meta.source.get("subagent")?.get("thread_spawn")?.clone();
    serde_json::from_value::<CodexSpawn>(spawn).ok().filter(|s| !s.parent_thread_id.is_empty() && !meta.id.is_empty())
}

fn codex_tail(text: &str) -> Tail {
    let mut tail = Tail::default();
    for line in text.lines() {
        let Ok(line) = serde_json::from_str::<CodexLine>(line) else { continue };
        let Ok(payload) = serde_json::from_value::<CodexPayload>(line.payload) else { continue };
        match (line.kind.as_str(), payload.kind.as_str()) {
            ("event_msg", kind) if CODEX_STARTED_EVENTS.contains(&kind) => tail.finished = false,
            ("event_msg", kind) if CODEX_FINISHED_EVENTS.contains(&kind) => tail.finished = true,
            ("event_msg", "agent_message") => {
                if let Some(message) = payload.message.filter(|m| !m.trim().is_empty()) {
                    tail.activity = single_line(message.lines().find(|l| !l.trim().is_empty()).unwrap_or_default());
                }
            }
            ("response_item", "function_call" | "custom_tool_call") => {
                let name = payload.name.unwrap_or_default();
                let input = payload.arguments.and_then(|a| serde_json::from_str::<Value>(&a).ok()).unwrap_or_default();
                tail.activity = summarize(&name, &input);
            }
            _ => {}
        }
    }
    tail
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::civil_date_from_ms;

    const SESSION: &str = "5bd0ccb0-0d09-47ca-9524-6eb740341fa9";

    struct Tree(PathBuf);

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tree(tag: &str) -> Tree {
        Tree(std::env::temp_dir().join(format!(
            "orkai-subagents-{tag}-{}-{}",
            std::process::id(),
            crate::util::now_ms()
        )))
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn lines(records: &[Value]) -> String {
        records.iter().map(|r| format!("{r}\n")).collect()
    }

    fn tool_use(name: &str, input: Value) -> Value {
        serde_json::json!({"type":"assistant","message":{"role":"assistant","stop_reason":null,"content":[{"type":"tool_use","name":name,"input":input}]}})
    }

    fn tool_result() -> Value {
        serde_json::json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}})
    }

    fn claude_parent(cwd: &Path, busy: bool) -> Parent {
        Parent {
            id: 7,
            kind: AgentKind::Claude,
            conversation_id: Some(SESSION.into()),
            cwd: cwd.to_path_buf(),
            created_at_ms: 0,
            busy,
        }
    }

    fn claude_subagent(dir: &Path, id: &str, meta: Value, records: &[Value]) {
        write(&dir.join(format!("agent-{id}.meta.json")), &meta.to_string());
        write(&dir.join(format!("agent-{id}.jsonl")), &lines(records));
    }

    #[test]
    fn claude_running_subagents_are_listed_and_finished_ones_hidden() {
        let t = tree("claude");
        let home = t.0.join("claude-home");
        let cwd = t.0.join("work").join("my_app");
        fs::create_dir_all(&cwd).unwrap();
        let dir = home.join("projects").join(claude_project_slug(&cwd)).join(SESSION).join("subagents");
        claude_subagent(
            &dir,
            "a1",
            serde_json::json!({"agentType":"Explore","description":"Map the   auth flow","requestShape":"foreground"}),
            &[
                tool_use("Read", serde_json::json!({"file_path":"src/auth.rs"})),
                tool_result(),
                tool_use("Bash", serde_json::json!({"command":"cargo test","description":"Run the tests"})),
            ],
        );
        claude_subagent(
            &dir,
            "a2",
            serde_json::json!({"agentType":"reviewer","description":"Review"}),
            &[
                tool_use("Grep", serde_json::json!({"pattern":"x"})),
                tool_use(CLAUDE_HANDBACK_TOOL, Value::Null),
                tool_result(),
            ],
        );
        claude_subagent(
            &dir,
            "a3",
            serde_json::json!({"agentType":"old"}),
            &[
                serde_json::json!({"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}),
            ],
        );
        claude_subagent(
            &dir,
            "a4",
            serde_json::json!({"description":"Stopped"}),
            &[
                tool_use("Read", Value::Null),
                serde_json::json!({"type":"attachment","attachment":{"hookEvent":"SubagentStop"}}),
            ],
        );

        let mut watcher = SubagentWatcher::new(Some(home.clone()), None);
        let found = watcher.scan(&[claude_parent(&cwd, true)], crate::util::now_ms());
        let subagents = &found[&7];
        assert_eq!(subagents.len(), 1, "{subagents:?}");
        assert_eq!(subagents[0].id, "a1");
        assert_eq!(subagents[0].role, "Explore");
        assert_eq!(subagents[0].description, "Map the auth flow");
        assert_eq!(subagents[0].activity, "Bash · Run the tests");

        let mut file = fs::OpenOptions::new().append(true).open(dir.join("agent-a1.jsonl")).unwrap();
        std::io::Write::write_all(&mut file, lines(&[tool_use(CLAUDE_HANDBACK_TOOL, Value::Null)]).as_bytes()).unwrap();
        drop(file);
        assert!(watcher.scan(&[claude_parent(&cwd, true)], crate::util::now_ms()).is_empty());
    }

    #[test]
    fn claude_foreground_subagents_need_a_busy_parent_but_background_ones_do_not() {
        let t = tree("shape");
        let home = t.0.join("claude-home");
        let cwd = t.0.join("project");
        fs::create_dir_all(&cwd).unwrap();
        let dir = home.join("projects").join(claude_project_slug(&cwd)).join(SESSION).join("subagents");
        claude_subagent(&dir, "fg", serde_json::json!({"agentType":"fg"}), &[tool_use("Read", Value::Null)]);
        claude_subagent(
            &dir,
            "bg",
            serde_json::json!({"agentType":"bg","requestShape":"background"}),
            &[tool_use("Read", Value::Null)],
        );
        let mut watcher = SubagentWatcher::new(Some(home), None);
        let now = crate::util::now_ms();
        let idle: Vec<String> =
            watcher.scan(&[claude_parent(&cwd, false)], now)[&7].iter().map(|s| s.role.clone()).collect();
        assert_eq!(idle, ["bg"]);
        assert_eq!(watcher.scan(&[claude_parent(&cwd, true)], now)[&7].len(), 2);
        assert!(watcher.scan(&[claude_parent(&cwd, true)], now + SUBAGENT_STALE_MS + 60_000).is_empty());
    }

    #[test]
    fn claude_session_folder_is_found_when_the_slug_differs() {
        let t = tree("slug");
        let home = t.0.join("claude-home");
        let cwd = t.0.join("elsewhere");
        let project = home.join("projects").join("hashed-project-name");
        write(&project.join(format!("{SESSION}.jsonl")), "{}\n");
        claude_subagent(
            &project.join(SESSION).join("subagents"),
            "x",
            serde_json::json!({}),
            &[tool_use("Read", Value::Null)],
        );
        let mut watcher = SubagentWatcher::new(Some(home), None);
        let found = watcher.scan(&[claude_parent(&cwd, true)], crate::util::now_ms());
        assert_eq!(found[&7][0].role, CLAUDE_DEFAULT_ROLE);
        assert_eq!(found[&7][0].activity, "Read");
    }

    #[test]
    fn claude_slug_replaces_every_non_alphanumeric_character() {
        assert_eq!(
            claude_project_slug(Path::new(r"C:\Users\aluca\orca\projects\orkai_cli")),
            "C--Users-aluca-orca-projects-orkai-cli"
        );
        assert_eq!(claude_project_slug(Path::new("/home/me/my.app")), "-home-me-my-app");
    }

    fn codex_rollout(day: &Path, name: &str, records: &[Value]) -> PathBuf {
        let path = day.join(format!("rollout-{name}.jsonl"));
        write(&path, &lines(records));
        path
    }

    fn session_meta(id: &str, source: Value) -> Value {
        serde_json::json!({"type":"session_meta","payload":{"id":id,"cwd":"/work","source":source}})
    }

    fn spawned_by(parent: &str) -> Value {
        serde_json::json!({"subagent":{"thread_spawn":{"parent_thread_id":parent,"depth":1,"agent_nickname":"Ada","agent_role":"explorer"}}})
    }

    fn event(kind: &str) -> Value {
        serde_json::json!({"type":"event_msg","payload":{"type":kind}})
    }

    #[test]
    fn codex_spawned_threads_of_the_parent_are_listed_until_complete() {
        let t = tree("codex");
        let codex = t.0.join("codex-home");
        let now = crate::util::now_ms();
        let (y, m, d) = civil_date_from_ms(now);
        let day = codex.join("sessions").join(format!("{y:04}")).join(format!("{m:02}")).join(format!("{d:02}"));
        codex_rollout(
            &day,
            "a-parent",
            &[session_meta("parent-1", Value::String("cli".into())), event("task_started")],
        );
        codex_rollout(
            &day,
            "b-child",
            &[
                session_meta("child-1", spawned_by("parent-1")),
                event("task_started"),
                serde_json::json!({"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"cargo test\"]}"}}),
            ],
        );
        codex_rollout(
            &day,
            "c-done",
            &[session_meta("child-2", spawned_by("parent-1")), event("task_started"), event("task_complete")],
        );
        codex_rollout(&day, "d-other", &[session_meta("child-3", spawned_by("someone-else")), event("task_started")]);
        let parent = Parent {
            id: 3,
            kind: AgentKind::Codex,
            conversation_id: Some("parent-1".into()),
            cwd: PathBuf::from("/work"),
            created_at_ms: now.saturating_sub(60_000),
            busy: false,
        };
        let mut watcher = SubagentWatcher::new(None, Some(codex));
        let found = watcher.scan(&[parent], now);
        let subagents = &found[&3];
        assert_eq!(subagents.len(), 1, "{subagents:?}");
        assert_eq!(subagents[0].role, "explorer");
        assert_eq!(subagents[0].description, "Ada");
        assert_eq!(subagents[0].activity, "shell · bash -lc cargo test");
    }

    #[test]
    fn each_codex_run_inside_a_terminal_resolves_its_own_thread() {
        let t = tree("codex-nested");
        let codex = t.0.join("codex-home");
        let now = crate::util::now_ms();
        let (y, m, d) = civil_date_from_ms(now);
        let day = codex.join("sessions").join(format!("{y:04}")).join(format!("{m:02}")).join(format!("{d:02}"));
        codex_rollout(&day, "a-first", &[session_meta("first-1", Value::String("cli".into()))]);
        codex_rollout(&day, "b-first-child", &[session_meta("child-1", spawned_by("first-1")), event("task_started")]);
        let first = Parent {
            id: 9,
            kind: AgentKind::Codex,
            conversation_id: None,
            cwd: PathBuf::from("/work"),
            created_at_ms: now.saturating_sub(30_000),
            busy: true,
        };
        let mut watcher = SubagentWatcher::new(None, Some(codex));
        let found = watcher.scan(std::slice::from_ref(&first), now);
        assert_eq!(found[&9].len(), 1);
        assert!(found[&9][0].id.ends_with("b-first-child"), "{:?}", found[&9]);

        codex_rollout(&day, "c-second", &[session_meta("second-1", Value::String("cli".into()))]);
        codex_rollout(
            &day,
            "d-second-child",
            &[session_meta("child-2", spawned_by("second-1")), event("task_started")],
        );
        let second = Parent { created_at_ms: now, ..first };
        let found = watcher.scan(&[second], now);
        assert_eq!(found[&9].len(), 1);
        assert!(found[&9][0].id.ends_with("d-second-child"), "{:?}", found[&9]);
    }

    #[test]
    fn missing_homes_and_terminals_have_no_subagents() {
        let parent = Parent {
            id: 1,
            kind: AgentKind::Terminal,
            conversation_id: None,
            cwd: PathBuf::from("/"),
            created_at_ms: 0,
            busy: true,
        };
        let mut watcher = SubagentWatcher::default();
        let claude = Parent { kind: AgentKind::Claude, conversation_id: Some(SESSION.into()), ..parent.clone() };
        let codex = Parent { kind: AgentKind::Codex, ..parent.clone() };
        assert!(watcher.scan(&[parent, claude, codex], crate::util::now_ms()).is_empty());
    }

    #[test]
    fn last_known_activity_survives_a_tail_without_tool_calls() {
        let t = tree("activity");
        let home = t.0.join("claude-home");
        let cwd = t.0.join("project");
        fs::create_dir_all(&cwd).unwrap();
        let dir = home.join("projects").join(claude_project_slug(&cwd)).join(SESSION).join("subagents");
        claude_subagent(
            &dir,
            "s",
            serde_json::json!({}),
            &[tool_use("Grep", serde_json::json!({"pattern":"fn draw","path":"src"}))],
        );
        let mut watcher = SubagentWatcher::new(Some(home), None);
        assert_eq!(watcher.scan(&[claude_parent(&cwd, true)], crate::util::now_ms())[&7][0].activity, "Grep · fn draw");
        let big = format!("{}\n", "y".repeat(usize::try_from(TAIL_BYTES).unwrap() * 2));
        let mut file = fs::OpenOptions::new().append(true).open(dir.join("agent-s.jsonl")).unwrap();
        std::io::Write::write_all(&mut file, big.as_bytes()).unwrap();
        drop(file);
        assert_eq!(watcher.scan(&[claude_parent(&cwd, true)], crate::util::now_ms())[&7][0].activity, "Grep · fn draw");
    }

    #[test]
    fn tails_skip_the_partial_first_line_of_large_transcripts() {
        let t = tree("tail");
        let path = t.0.join("big.jsonl");
        let filler = "x".repeat(usize::try_from(TAIL_BYTES).unwrap());
        write(&path, &format!("{filler}\n{}", lines(&[tool_use("Edit", serde_json::json!({"file_path":"a.rs"}))])));
        let len = fs::metadata(&path).unwrap().len();
        let text = read_tail(&path, len).unwrap();
        assert_eq!(claude_tail(&text).activity, "Edit · a.rs");
    }
}
