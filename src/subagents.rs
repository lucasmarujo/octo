use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::fsutil;
use crate::holder::nested::{ClaudeSession, claude_sessions_among, descendants};
use crate::launch::{find_codex_session, is_safe_session_id, recent_codex_rollouts};
use crate::model::{AgentId, AgentKind, AgentRecord, AgentState, NestedAgent};
use crate::util::is_uuid;

pub const SUBAGENT_STALE_MS: u64 = 10 * 60 * 1_000;
pub const MAX_SUBAGENTS: usize = 64;
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
const CLAUDE_ASSISTANT: &str = "assistant";
const CLAUDE_SYNTHETIC_MODEL: &str = "<synthetic>";
const CODEX_TURN_CONTEXT: &str = "turn_context";
const MODEL_FIELD: &str = "\"model\"";
const MODEL_WINDOW_BYTES: u64 = 1024 * 1024;
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

fn most_recent(mut subagents: Vec<Subagent>) -> Vec<Subagent> {
    subagents.sort_by(|a, b| a.started_at_ms.cmp(&b.started_at_ms).then_with(|| a.id.cmp(&b.id)));
    subagents.drain(..subagents.len().saturating_sub(MAX_SUBAGENTS));
    subagents
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parent {
    pub id: AgentId,
    pub kind: AgentKind,
    pub conversation_id: Option<String>,
    pub cwd: PathBuf,
    pub created_at_ms: u64,
    pub busy: bool,
    pub pid: Option<u32>,
}

pub fn parent_of(
    record: &AgentRecord,
    kind: AgentKind,
    nested: Option<&NestedAgent>,
    state: AgentState,
    child_pid: Option<u32>,
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
        pid: nested.map_or(child_pid, |n| Some(n.pid)),
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

struct CachedModel {
    len: u64,
    modified: u64,
    model: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ModelField {
    model: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeModelLine {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "isSidechain")]
    sidechain: bool,
    message: ModelField,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexModelLine {
    #[serde(rename = "type")]
    kind: String,
    payload: ModelField,
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
    codex_rollouts: HashMap<String, Option<PathBuf>>,
    models: HashMap<PathBuf, CachedModel>,
}

impl SubagentWatcher {
    pub fn new(claude_home: Option<PathBuf>, codex_home: Option<PathBuf>) -> Self {
        Self { claude_home, codex_home, ..Self::default() }
    }

    pub fn scan(&mut self, parents: &[Parent], now: u64) -> BTreeMap<AgentId, Vec<Subagent>> {
        let mut found = BTreeMap::new();
        for parent in parents {
            let subagents = match parent.kind {
                AgentKind::Claude => self.claude(parent, now),
                AgentKind::Codex => self.codex(parent, now),
                AgentKind::Kilo
                | AgentKind::Opencode
                | AgentKind::Antigravity
                | AgentKind::Grok
                | AgentKind::Qwen
                | AgentKind::Kimi
                | AgentKind::Pi
                | AgentKind::Cursor
                | AgentKind::Terminal => Vec::new(),
            };
            if subagents.is_empty() {
                continue;
            }
            found.insert(parent.id, most_recent(subagents));
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

    pub fn models(&mut self, parents: &[Parent]) -> BTreeMap<AgentId, String> {
        let mut found = BTreeMap::new();
        for parent in parents {
            let model = match parent.kind {
                AgentKind::Claude => self.claude_model(parent),
                AgentKind::Codex => self.codex_model(parent),
                AgentKind::Kilo
                | AgentKind::Opencode
                | AgentKind::Antigravity
                | AgentKind::Grok
                | AgentKind::Qwen
                | AgentKind::Kimi
                | AgentKind::Pi
                | AgentKind::Cursor
                | AgentKind::Terminal => None,
            };
            if let Some(model) = model {
                found.insert(parent.id, model);
            }
        }
        if self.models.len() > MAX_CACHE_ENTRIES {
            self.models.clear();
        }
        if self.codex_rollouts.len() > MAX_CACHE_ENTRIES {
            self.codex_rollouts.clear();
        }
        found
    }

    fn claude_session(&self, parent: &Parent) -> Option<String> {
        let live = self
            .claude_home
            .as_deref()
            .zip(parent.pid)
            .and_then(|(home, pid)| crate::holder::nested::claude_session(home, pid));
        live.or_else(|| parent.conversation_id.clone()).filter(|id| is_uuid(id))
    }

    fn claude_model(&mut self, parent: &Parent) -> Option<String> {
        let session_id = self.claude_session(parent)?;
        let session = self.claude_dirs.get(&session_id)?.parent()?;
        let path = session.with_file_name(format!("{session_id}{CLAUDE_TRANSCRIPT_SUFFIX}"));
        self.last_model(&path, claude_model)
    }

    fn codex_model(&mut self, parent: &Parent) -> Option<String> {
        let codex_home = self.codex_home.clone()?;
        let key = (parent.id, parent.created_at_ms);
        let thread = parent
            .conversation_id
            .clone()
            .or_else(|| self.codex_threads.get(&key).cloned())
            .filter(|id| is_safe_session_id(id))?;
        let rollout = self.codex_rollouts.entry(thread).or_insert_with_key(|thread| {
            let name = format!("-{thread}.jsonl");
            recent_codex_rollouts(&codex_home, parent.created_at_ms)
                .into_iter()
                .find(|path| path.to_string_lossy().ends_with(&name))
        });
        let path = rollout.clone()?;
        self.last_model(&path, codex_model)
    }

    fn last_model(&mut self, path: &Path, parse: fn(&str) -> Option<String>) -> Option<String> {
        let (len, modified, _) = stamp(path)?;
        let known = self.models.get(path);
        if let Some(cached) = known.filter(|c| c.len == len && c.modified == modified) {
            return cached.model.clone();
        }
        let previous = known.and_then(|cached| cached.model.clone());
        let seen = known.map_or(0, |cached| if cached.len <= len { cached.len } else { 0 });
        let start = seen.saturating_sub(1).max(len.saturating_sub(MODEL_WINDOW_BYTES));
        let Some(text) = read_window(path, start, MODEL_WINDOW_BYTES) else { return previous };
        let read = parse(&text).map(|model| single_line(&model)).filter(|model| !model.is_empty());
        let model = read.or(previous);
        self.models.insert(path.to_path_buf(), CachedModel { len, modified, model: model.clone() });
        model
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

    fn background_sessions(&self, parent: &Parent) -> Vec<ClaudeSession> {
        let Some((home, pid)) = self.claude_home.as_deref().zip(parent.pid) else { return Vec::new() };
        claude_sessions_among(home, &descendants(pid))
    }

    fn claude(&mut self, parent: &Parent, now: u64) -> Vec<Subagent> {
        let mut sessions: Vec<ClaudeSession> = self
            .claude_session(parent)
            .map(|session_id| ClaudeSession { session_id, cwd: None, busy: parent.busy })
            .into_iter()
            .collect();
        for session in self.background_sessions(parent) {
            if is_uuid(&session.session_id) && !sessions.iter().any(|known| known.session_id == session.session_id) {
                sessions.push(session);
            }
        }
        let mut found: Vec<Subagent> = Vec::new();
        for session in sessions {
            let cwd = session.cwd.clone().unwrap_or_else(|| parent.cwd.clone());
            for subagent in self.claude_in_session(&session.session_id, &cwd, session.busy, now) {
                if !found.iter().any(|known| known.id == subagent.id) {
                    found.push(subagent);
                }
            }
        }
        found
    }

    fn claude_in_session(&mut self, session_id: &str, cwd: &Path, busy: bool, now: u64) -> Vec<Subagent> {
        let Some(dir) = self.claude_dir(session_id, cwd) else { return Vec::new() };
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
            if !busy && meta.request_shape.as_deref() != Some(CLAUDE_BACKGROUND_SHAPE) {
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
    read_window(path, len.saturating_sub(TAIL_BYTES), TAIL_BYTES)
}

fn read_window(path: &Path, start: u64, limit: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).ok()?;
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

fn claude_model(text: &str) -> Option<String> {
    text.lines().rev().filter(|line| line.contains(MODEL_FIELD)).find_map(|line| {
        let record: ClaudeModelLine = serde_json::from_str(line).ok()?;
        if record.kind != CLAUDE_ASSISTANT || record.sidechain {
            return None;
        }
        record.message.model.filter(|model| model != CLAUDE_SYNTHETIC_MODEL)
    })
}

fn codex_model(text: &str) -> Option<String> {
    text.lines().rev().filter(|line| line.contains(MODEL_FIELD)).find_map(|line| {
        let record: CodexModelLine = serde_json::from_str(line).ok()?;
        record.payload.model.filter(|_| record.kind == CODEX_TURN_CONTEXT)
    })
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
            pid: None,
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
    fn a_long_list_of_subagents_keeps_the_most_recent_in_the_order_they_started() {
        let subagent = |started_at_ms: u64| Subagent {
            id: format!("a{started_at_ms}"),
            role: "Explore".into(),
            description: String::new(),
            activity: String::new(),
            started_at_ms,
        };
        let total = u64::try_from(MAX_SUBAGENTS).unwrap() + 3;
        let kept = most_recent((1..=total).rev().map(subagent).collect());
        assert_eq!(kept.len(), MAX_SUBAGENTS);
        assert_eq!(kept.first().map(|s| s.started_at_ms), Some(4), "the oldest ones leave first");
        assert_eq!(kept.last().map(|s| s.started_at_ms), Some(total), "the newest stays");

        let few = most_recent(vec![subagent(2), subagent(1)]);
        assert_eq!(few.iter().map(|s| s.started_at_ms).collect::<Vec<_>>(), [1, 2]);
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
    fn a_cleared_or_resumed_claude_lists_the_subagents_of_the_session_it_runs_now() {
        const LIVE: &str = "0d6c1f7e-4b8a-4f3e-9a51-2c7d8e9f0a1b";
        const PID: u32 = 4242;
        let t = tree("live");
        let home = t.0.join("claude-home");
        let cwd = t.0.join("project");
        fs::create_dir_all(&cwd).unwrap();
        let project = home.join("projects").join(claude_project_slug(&cwd));
        claude_subagent(
            &project.join(SESSION).join("subagents"),
            "old",
            serde_json::json!({"agentType":"launched"}),
            &[tool_use("Read", Value::Null)],
        );
        claude_subagent(
            &project.join(LIVE).join("subagents"),
            "new",
            serde_json::json!({"agentType":"after-clear"}),
            &[tool_use("Grep", Value::Null)],
        );
        let now = crate::util::now_ms();
        let running = Parent { pid: Some(PID), ..claude_parent(&cwd, true) };
        let roles = |watcher: &mut SubagentWatcher| -> Vec<String> {
            watcher
                .scan(std::slice::from_ref(&running), now)
                .remove(&7)
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.role)
                .collect()
        };

        let mut watcher = SubagentWatcher::new(Some(home.clone()), None);
        assert_eq!(roles(&mut watcher), ["launched"], "without a session file the launched session is the one");

        let registry = home.join("sessions").join(format!("{PID}.json"));
        write(&registry, &serde_json::json!({"pid": PID, "sessionId": LIVE, "cwd": cwd}).to_string());
        assert_eq!(roles(&mut watcher), ["after-clear"], "the session Claude reports for its process wins");

        write(&registry, &serde_json::json!({"pid": PID + 1, "sessionId": LIVE}).to_string());
        assert_eq!(roles(&mut watcher), ["launched"], "a file left by another process is ignored");
    }

    struct Child(std::process::Child);

    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn long_running_child() -> Child {
        let mut command = if cfg!(windows) {
            let mut ping = std::process::Command::new("ping");
            ping.args(["-n", "60", "127.0.0.1"]);
            ping
        } else {
            let mut sleep = std::process::Command::new("sleep");
            sleep.arg("60");
            sleep
        };
        Child(command.stdout(std::process::Stdio::null()).spawn().unwrap())
    }

    #[test]
    fn subagents_of_a_background_session_claude_runs_under_the_agent_are_listed() {
        const BACKGROUND: &str = "c82c00b5-5f5b-44d0-a322-f806f88bf804";
        let t = tree("background");
        let home = t.0.join("claude-home");
        let cwd = t.0.join("project");
        fs::create_dir_all(&cwd).unwrap();
        let project = home.join("projects").join(claude_project_slug(&cwd));
        write(&project.join(format!("{SESSION}{CLAUDE_TRANSCRIPT_SUFFIX}")), "{}\n");
        let subagents = project.join(BACKGROUND).join("subagents");
        claude_subagent(&subagents, "fg", serde_json::json!({"agentType":"Explore"}), &[tool_use("Grep", Value::Null)]);
        claude_subagent(
            &subagents,
            "bg",
            serde_json::json!({"agentType":"general-purpose","requestShape":"background"}),
            &[tool_use("Bash", Value::Null)],
        );
        let spare = long_running_child();
        let registry = home.join("sessions").join(format!("{}.json", spare.0.id()));
        let session = |status: &str| {
            serde_json::json!({"pid": spare.0.id(), "sessionId": BACKGROUND, "cwd": cwd, "status": status}).to_string()
        };
        let agent = Parent { pid: Some(std::process::id()), ..claude_parent(&cwd, false) };
        let now = crate::util::now_ms();
        let roles = |watcher: &mut SubagentWatcher| -> Vec<String> {
            let mut roles: Vec<String> = watcher
                .scan(std::slice::from_ref(&agent), now)
                .remove(&7)
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.role)
                .collect();
            roles.sort();
            roles
        };
        let mut watcher = SubagentWatcher::new(Some(home.clone()), None);
        assert!(roles(&mut watcher).is_empty(), "no session file, no background session");

        write(&registry, &session("busy"));
        assert_eq!(
            roles(&mut watcher),
            ["Explore", "general-purpose"],
            "the busy background session shows all its work"
        );

        write(&registry, &session("idle"));
        assert_eq!(
            roles(&mut watcher),
            ["general-purpose"],
            "an idle background session keeps only its background subagents"
        );
    }

    #[test]
    fn the_process_of_a_parent_is_the_agent_nested_in_a_terminal_or_else_the_one_launched() {
        let record: AgentRecord =
            serde_json::from_str(r#"{"id":5,"session":"s","name":"a","kind":"terminal","cwd":"/x","created_at_ms":1}"#)
                .unwrap();
        let nested = NestedAgent {
            kind: AgentKind::Claude,
            pid: 77,
            conversation_id: Some(SESSION.into()),
            cwd: None,
            since_ms: 9,
        };
        let inside = parent_of(&record, AgentKind::Claude, Some(&nested), AgentState::Working, Some(11)).unwrap();
        assert_eq!(inside.pid, Some(77));
        assert_eq!(inside.conversation_id.as_deref(), Some(SESSION));
        let launched = parent_of(&record, AgentKind::Claude, None, AgentState::Idle, Some(11)).unwrap();
        assert_eq!(launched.pid, Some(11));
        assert!(parent_of(&record, AgentKind::Terminal, None, AgentState::Working, Some(11)).is_none());
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
            pid: None,
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
            pid: None,
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
            pid: None,
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

    fn assistant_said(model: &str) -> Value {
        serde_json::json!({"type":"assistant","message":{"model":model,"role":"assistant","content":[{"type":"text","text":"hi"}]}})
    }

    fn append(path: &Path, text: &str) {
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        std::io::Write::write_all(&mut file, text.as_bytes()).unwrap();
    }

    fn claude_transcript(t: &Tree, records: &[Value]) -> (PathBuf, PathBuf, PathBuf) {
        let home = t.0.join("claude-home");
        let cwd = t.0.join("project");
        fs::create_dir_all(&cwd).unwrap();
        let path = home.join("projects").join(claude_project_slug(&cwd)).join(format!("{SESSION}.jsonl"));
        write(&path, &lines(records));
        (home, cwd, path)
    }

    fn claude_model_after_scan(watcher: &mut SubagentWatcher, cwd: &Path) -> Option<String> {
        let parents = [claude_parent(cwd, true)];
        watcher.scan(&parents, crate::util::now_ms());
        watcher.models(&parents).remove(&7)
    }

    #[test]
    fn the_model_of_a_claude_session_is_the_last_one_that_really_answered() {
        let t = tree("model-claude");
        let (home, cwd, path) = claude_transcript(
            &t,
            &[
                assistant_said("claude-sonnet-4-5"),
                assistant_said("claude-opus-5-5"),
                assistant_said(CLAUDE_SYNTHETIC_MODEL),
                serde_json::json!({"type":"assistant","isSidechain":true,"message":{"model":"claude-haiku-4-5"}}),
                serde_json::json!({"type":"user","message":{"role":"user","model":"not-an-answer"}}),
                tool_result(),
            ],
        );
        let mut watcher = SubagentWatcher::new(Some(home), None);
        assert_eq!(claude_model_after_scan(&mut watcher, &cwd).as_deref(), Some("claude-opus-5-5"));

        append(&path, &lines(&[assistant_said("claude-sonnet-4-5")]));
        assert_eq!(
            claude_model_after_scan(&mut watcher, &cwd).as_deref(),
            Some("claude-sonnet-4-5"),
            "/model shows up"
        );
    }

    #[test]
    fn a_claude_session_that_has_not_answered_has_no_model() {
        let t = tree("model-none");
        let (home, cwd, _) = claude_transcript(&t, &[tool_result(), assistant_said(CLAUDE_SYNTHETIC_MODEL)]);
        let mut watcher = SubagentWatcher::new(Some(home), None);
        assert_eq!(claude_model_after_scan(&mut watcher, &cwd), None);
        let unknown = Parent { conversation_id: None, ..claude_parent(&cwd, true) };
        assert!(watcher.models(&[unknown]).is_empty());
    }

    #[test]
    fn reading_the_model_is_bounded_and_only_looks_at_what_was_written_since() {
        let t = tree("model-bounded");
        let (home, cwd, path) = claude_transcript(&t, &[assistant_said("claude-too-far")]);
        let filler = format!("{}\n", "x".repeat(usize::try_from(MODEL_WINDOW_BYTES).unwrap() + 1));
        append(&path, &filler);
        let mut watcher = SubagentWatcher::new(Some(home), None);
        assert_eq!(claude_model_after_scan(&mut watcher, &cwd), None, "a cold start never reads past its window");

        append(&path, &lines(&[assistant_said("claude-opus-5-5")]));
        assert_eq!(claude_model_after_scan(&mut watcher, &cwd).as_deref(), Some("claude-opus-5-5"));
        assert_eq!(watcher.models[&path].len, fs::metadata(&path).unwrap().len());

        append(&path, &filler);
        append(&path, &filler);
        assert_eq!(
            claude_model_after_scan(&mut watcher, &cwd).as_deref(),
            Some("claude-opus-5-5"),
            "kept without news"
        );

        let len = fs::metadata(&path).unwrap().len();
        let window = read_window(&path, len.saturating_sub(MODEL_WINDOW_BYTES), MODEL_WINDOW_BYTES).unwrap();
        assert!(u64::try_from(window.len()).unwrap() <= MODEL_WINDOW_BYTES);
    }

    #[test]
    fn a_change_is_read_from_where_the_last_read_stopped_and_not_again_from_the_tail() {
        let t = tree("model-delta");
        let (home, cwd, path) = claude_transcript(&t, &[assistant_said("claude-opus-5-5")]);
        let mut watcher = SubagentWatcher::new(Some(home), None);
        assert_eq!(claude_model_after_scan(&mut watcher, &cwd).as_deref(), Some("claude-opus-5-5"));

        let cached = watcher.models.get_mut(&path).unwrap();
        cached.model = Some("from-the-cache".into());
        append(&path, &lines(&[tool_result()]));
        assert_eq!(
            claude_model_after_scan(&mut watcher, &cwd).as_deref(),
            Some("from-the-cache"),
            "old bytes stay unread"
        );
    }

    #[test]
    fn an_answer_longer_than_the_tail_of_the_subagents_is_still_read_on_a_cold_start() {
        let t = tree("model-large");
        let text = "y".repeat(usize::try_from(TAIL_BYTES).unwrap() * 3);
        let large = serde_json::json!({"type":"assistant","message":{"model":"claude-opus-5-5","content":[{"type":"text","text":text}]}});
        let (home, cwd, _) = claude_transcript(&t, &[large]);
        let mut watcher = SubagentWatcher::new(Some(home), None);
        assert_eq!(claude_model_after_scan(&mut watcher, &cwd).as_deref(), Some("claude-opus-5-5"));
    }

    #[test]
    fn the_model_of_a_codex_session_comes_from_the_turn_context_of_its_own_rollout() {
        let t = tree("model-codex");
        let codex = t.0.join("codex-home");
        let now = crate::util::now_ms();
        let (y, m, d) = civil_date_from_ms(now);
        let day = codex.join("sessions").join(format!("{y:04}")).join(format!("{m:02}")).join(format!("{d:02}"));
        let turn = |model: &str| serde_json::json!({"type":"turn_context","payload":{"cwd":"/work","model":model,"effort":"high"}});
        let own = codex_rollout(
            &day,
            "2026-11-06T10-00-00-parent-1",
            &[
                session_meta("parent-1", Value::String("cli".into())),
                turn("gpt-5-codex"),
                turn("gpt-5.1-codex-max"),
                serde_json::json!({"type":"response_item","payload":{"type":"message","model":"not-a-turn"}}),
                event("task_started"),
            ],
        );
        codex_rollout(
            &day,
            "2026-11-06T10-00-01-parent-2",
            &[session_meta("parent-2", Value::String("cli".into())), turn("o3")],
        );
        let parent = Parent {
            id: 3,
            kind: AgentKind::Codex,
            conversation_id: Some("parent-1".into()),
            cwd: PathBuf::from("/work"),
            created_at_ms: now.saturating_sub(60_000),
            busy: true,
            pid: None,
        };
        let without_rollout = Parent { id: 4, conversation_id: Some("parent-3".into()), ..parent.clone() };
        let mut watcher = SubagentWatcher::new(None, Some(codex));
        let found = watcher.models(&[parent.clone(), without_rollout]);
        assert_eq!(found.get(&3).map(String::as_str), Some("gpt-5.1-codex-max"));
        assert!(!found.contains_key(&4));

        append(&own, &lines(&[turn("gpt-5")]));
        assert_eq!(watcher.models(&[parent]).get(&3).map(String::as_str), Some("gpt-5"));
    }

    #[test]
    fn kinds_without_a_reliable_source_report_no_model() {
        let t = tree("model-kinds");
        let (home, cwd, _) = claude_transcript(&t, &[assistant_said("claude-opus-5-5")]);
        let mut watcher = SubagentWatcher::new(Some(home), None);
        claude_model_after_scan(&mut watcher, &cwd);
        for kind in AgentKind::ALL.into_iter().filter(|kind| !matches!(kind, AgentKind::Claude | AgentKind::Codex)) {
            assert!(watcher.models(&[Parent { kind, ..claude_parent(&cwd, true) }]).is_empty(), "{kind:?}");
        }
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
