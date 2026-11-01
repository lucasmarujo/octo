use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub type AgentId = u32;

pub const DEFAULT_SESSION: &str = "default";
pub const MAX_SESSION_NAME_LEN: usize = 32;
pub const MAX_AGENT_NAME_LEN: usize = 40;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    Kilo,
    Opencode,
    Antigravity,
    Grok,
    Qwen,
    Kimi,
    Pi,
    Terminal,
}

impl AgentKind {
    pub const ALL: [AgentKind; 10] = [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Kilo,
        AgentKind::Opencode,
        AgentKind::Antigravity,
        AgentKind::Grok,
        AgentKind::Qwen,
        AgentKind::Kimi,
        AgentKind::Pi,
        AgentKind::Terminal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Kilo => "kilo",
            AgentKind::Opencode => "opencode",
            AgentKind::Antigravity => "agy",
            AgentKind::Grok => "grok",
            AgentKind::Qwen => "qwen",
            AgentKind::Kimi => "kimi",
            AgentKind::Pi => "pi",
            AgentKind::Terminal => "terminal",
        }
    }

    pub fn is_ai(self) -> bool {
        !matches!(self, AgentKind::Terminal)
    }

    fn position(self) -> usize {
        AgentKind::ALL.iter().position(|kind| *kind == self).unwrap_or(0)
    }

    pub fn next(self) -> Self {
        AgentKind::ALL[(self.position() + 1) % AgentKind::ALL.len()]
    }

    pub fn prev(self) -> Self {
        AgentKind::ALL[(self.position() + AgentKind::ALL.len() - 1) % AgentKind::ALL.len()]
    }

    pub fn names() -> String {
        AgentKind::ALL.iter().map(|kind| kind.label()).collect::<Vec<_>>().join("|")
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "claude" | "cc" => Some(AgentKind::Claude),
            "codex" => Some(AgentKind::Codex),
            "kilo" | "kilocode" => Some(AgentKind::Kilo),
            "opencode" | "oc" => Some(AgentKind::Opencode),
            "agy" | "antigravity" | "gemini" => Some(AgentKind::Antigravity),
            "grok" => Some(AgentKind::Grok),
            "qwen" => Some(AgentKind::Qwen),
            "kimi" => Some(AgentKind::Kimi),
            "pi" => Some(AgentKind::Pi),
            "terminal" | "term" | "shell" => Some(AgentKind::Terminal),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Idle,
    Working,
    Paused,
    Done,
    Exited,
    Lost,
}

impl AgentState {
    pub fn label(self) -> &'static str {
        match self {
            AgentState::Idle => "idle",
            AgentState::Working => "working",
            AgentState::Paused => "paused",
            AgentState::Done => "done",
            AgentState::Exited => "exited",
            AgentState::Lost => "lost",
        }
    }

    pub fn is_alive(self) -> bool {
        !matches!(self, AgentState::Exited | AgentState::Lost)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub repo: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
    pub source: PathBuf,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentRecord {
    pub id: AgentId,
    pub session: String,
    pub name: String,
    pub kind: AgentKind,
    pub cwd: PathBuf,
    #[serde(default)]
    pub task: String,
    pub created_at_ms: u64,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub launches: u32,
    #[serde(default)]
    pub worktree: Option<Worktree>,
    #[serde(default)]
    pub bypass: bool,
    #[serde(default = "started_before_remote_control_was_a_choice")]
    pub remote_control: bool,
    #[serde(default)]
    pub project: Option<PathBuf>,
    #[serde(default)]
    pub parent: Option<AgentId>,
    #[serde(default)]
    pub race: Option<u64>,
}

fn started_before_remote_control_was_a_choice() -> bool {
    true
}

impl AgentRecord {
    pub fn project_dir(&self) -> &Path {
        self.worktree
            .as_ref()
            .map(|worktree| worktree.source.as_path())
            .or(self.project.as_deref())
            .unwrap_or(&self.cwd)
    }

    pub fn last_used_ms(&self, status: Option<&StatusReport>) -> u64 {
        let Some(status) = status else { return self.created_at_ms };
        let turn = status.running_kind(self.kind).is_ai().then(|| status.last_turn_started_ms()).flatten();
        turn.into_iter().fold(self.created_at_ms.max(status.input_at_ms), u64::max)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NewAgent {
    pub session: String,
    pub name: String,
    pub kind: AgentKind,
    pub cwd: PathBuf,
    pub task: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LaunchSpec {
    pub kind: AgentKind,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub env_remove: Vec<String>,
    #[serde(default)]
    pub initial_input: Option<String>,
    pub rows: u16,
    pub cols: u16,
    #[serde(default)]
    pub test: Option<TestTarget>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TestTarget {
    pub repo: PathBuf,
    pub dir: PathBuf,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TestState {
    Running,
    Passed,
    Failed,
    Untrusted,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TestReport {
    pub state: TestState,
    pub command: String,
    pub started_at_ms: u64,
    #[serde(default)]
    pub finished_at_ms: Option<u64>,
    #[serde(default)]
    pub code: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInfo {
    pub holder_pid: u32,
    #[serde(default)]
    pub child_pid: Option<u32>,
    pub port: u16,
    pub token: String,
    pub started_at_ms: u64,
    pub protocol: u32,
    pub version: String,
    #[serde(default)]
    pub exe: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ExitInfo {
    #[serde(default)]
    pub code: Option<u32>,
    #[serde(default)]
    pub error: Option<String>,
    pub at_ms: u64,
}

impl ExitInfo {
    pub fn describe(&self) -> String {
        match (&self.error, self.code) {
            (Some(error), _) => format!("failed: {error}"),
            (None, Some(code)) => format!("exited with code {code}"),
            (None, None) => "exited".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NestedAgent {
    pub kind: AgentKind,
    pub pid: u32,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    pub since_ms: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StatusReport {
    pub state: AgentState,
    pub state_since_ms: u64,
    #[serde(default)]
    pub turn_started_ms: Option<u64>,
    #[serde(default)]
    pub last_turn_ms: Option<u64>,
    pub started_at_ms: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub activity: String,
    pub rows: u16,
    pub cols: u16,
    #[serde(default)]
    pub exit: Option<ExitInfo>,
    #[serde(default)]
    pub nested: Option<NestedAgent>,
    #[serde(default)]
    pub viewers: Option<u16>,
    #[serde(default)]
    pub test: Option<Box<TestReport>>,
    #[serde(default)]
    pub input_at_ms: u64,
}

impl StatusReport {
    pub fn running_kind(&self, launched: AgentKind) -> AgentKind {
        self.nested.as_ref().map_or(launched, |nested| nested.kind)
    }

    pub fn last_turn_started_ms(&self) -> Option<u64> {
        self.turn_started_ms.or_else(|| self.last_turn_ms.map(|took| self.state_since_ms.saturating_sub(took)))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub created_at_ms: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct SessionList {
    pub sessions: Vec<Session>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UiState {
    pub session: String,
    pub who: AgentKind,
    #[serde(default)]
    pub where_dir: Option<PathBuf>,
    #[serde(default)]
    pub tabs: Vec<AgentId>,
    #[serde(default)]
    pub session_dirs: BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub worktree: bool,
    #[serde(default)]
    pub bypass: bool,
    #[serde(default)]
    pub pwa: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            session: DEFAULT_SESSION.to_string(),
            who: AgentKind::Claude,
            where_dir: None,
            tabs: Vec::new(),
            session_dirs: BTreeMap::new(),
            worktree: false,
            bypass: false,
            pwa: false,
        }
    }
}

pub fn validate_session_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("session name cannot be empty".into());
    }
    if trimmed.chars().count() > MAX_SESSION_NAME_LEN {
        return Err(format!("session name must have at most {MAX_SESSION_NAME_LEN} characters"));
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("session name may only contain letters, digits, '-', '_' and '.'".into());
    }
    Ok(trimmed.to_string())
}

pub fn validate_agent_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("agent name cannot be empty".into());
    }
    if trimmed.chars().count() > MAX_AGENT_NAME_LEN {
        return Err(format!("agent name must have at most {MAX_AGENT_NAME_LEN} characters"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err("agent name cannot contain control characters".into());
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_cycles_through_all_values() {
        let mut kind = AgentKind::Claude;
        let mut seen = Vec::new();
        for _ in 0..AgentKind::ALL.len() {
            seen.push(kind);
            kind = kind.next();
        }
        assert_eq!(kind, AgentKind::Claude);
        assert_eq!(seen, AgentKind::ALL);
        assert_eq!(AgentKind::Claude.prev(), AgentKind::Terminal);
        assert_eq!(AgentKind::Terminal.next(), AgentKind::Claude);
        assert_eq!(AgentKind::Codex.next(), AgentKind::Kilo);
        assert_eq!(AgentKind::Kimi.next(), AgentKind::Pi);
        assert_eq!(AgentKind::Pi.next(), AgentKind::Terminal);
        for kind in AgentKind::ALL {
            assert_eq!(kind.next().prev(), kind);
        }
    }

    #[test]
    fn kind_parses_aliases() {
        assert_eq!(AgentKind::parse("Claude"), Some(AgentKind::Claude));
        assert_eq!(AgentKind::parse("shell"), Some(AgentKind::Terminal));
        assert_eq!(AgentKind::parse("codex"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::parse("vim"), None);
        for kind in AgentKind::ALL {
            assert_eq!(AgentKind::parse(kind.label()), Some(kind));
        }
        assert_eq!(AgentKind::parse("KiloCode"), Some(AgentKind::Kilo));
        assert_eq!(AgentKind::parse("oc"), Some(AgentKind::Opencode));
        assert_eq!(AgentKind::parse("gemini"), Some(AgentKind::Antigravity));
        assert_eq!(AgentKind::parse("Antigravity"), Some(AgentKind::Antigravity));
        assert_eq!(AgentKind::parse("Pi"), Some(AgentKind::Pi));
        assert_eq!(AgentKind::names(), "claude|codex|kilo|opencode|agy|grok|qwen|kimi|pi|terminal");
    }

    #[test]
    fn kind_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&AgentKind::Terminal).unwrap(), "\"terminal\"");
        assert_eq!(serde_json::to_string(&AgentKind::Opencode).unwrap(), "\"opencode\"");
        assert_eq!(serde_json::from_str::<AgentKind>("\"kimi\"").unwrap(), AgentKind::Kimi);
        assert_eq!(serde_json::to_string(&AgentKind::Antigravity).unwrap(), "\"antigravity\"");
        assert_eq!(serde_json::to_string(&AgentState::Paused).unwrap(), "\"paused\"");
    }

    #[test]
    fn session_names_are_validated() {
        assert_eq!(validate_session_name(" work-1 ").unwrap(), "work-1");
        assert!(validate_session_name("").is_err());
        assert!(validate_session_name("has space").is_err());
        assert!(validate_session_name("../escape").is_err());
        assert!(validate_session_name(&"a".repeat(MAX_SESSION_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn agent_names_are_validated() {
        assert_eq!(validate_agent_name(" localhost-server ").unwrap(), "localhost-server");
        assert!(validate_agent_name("   ").is_err());
        assert!(validate_agent_name("bad\nname").is_err());
        assert!(validate_agent_name(&"x".repeat(MAX_AGENT_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn record_deserializes_with_missing_optional_fields() {
        let json = r#"{"id":3,"session":"default","name":"a","kind":"claude","cwd":"/x","created_at_ms":1}"#;
        let record: AgentRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.task, "");
        assert_eq!(record.conversation_id, None);
        assert_eq!(record.launches, 0);
        assert!(!record.bypass, "agents saved before bypass existed keep asking for permissions");
        assert!(record.remote_control, "agents saved before it was a choice keep the remote control they started with");
        let chosen = r#"{"id":3,"session":"default","name":"a","kind":"claude","cwd":"/x","created_at_ms":1,"remote_control":false}"#;
        assert!(!serde_json::from_str::<AgentRecord>(chosen).unwrap().remote_control);
    }

    #[test]
    fn status_without_nested_agent_keeps_the_launched_kind() {
        let json = r#"{"state":"idle","state_since_ms":1,"started_at_ms":1,"rows":24,"cols":80}"#;
        let mut status: StatusReport = serde_json::from_str(json).unwrap();
        assert_eq!(status.nested, None);
        assert_eq!(status.running_kind(AgentKind::Terminal), AgentKind::Terminal);
        status.nested =
            Some(NestedAgent { kind: AgentKind::Codex, pid: 42, conversation_id: None, cwd: None, since_ms: 5 });
        assert_eq!(status.running_kind(AgentKind::Terminal), AgentKind::Codex);
    }

    #[test]
    fn an_agent_was_last_used_when_it_started_got_typed_into_or_began_a_turn() {
        let record: AgentRecord = serde_json::from_str(
            r#"{"id":1,"session":"s","name":"n","kind":"claude","cwd":"/x","created_at_ms":1000}"#,
        )
        .unwrap();
        assert_eq!(record.last_used_ms(None), 1_000, "an agent orkai has not heard from yet");
        let mut status: StatusReport =
            serde_json::from_str(r#"{"state":"idle","state_since_ms":1000,"started_at_ms":1000,"rows":24,"cols":80}"#)
                .unwrap();
        assert_eq!(status.input_at_ms, 0, "a holder of an older orkai does not report input");
        assert_eq!(record.last_used_ms(Some(&status)), 1_000);

        status.input_at_ms = 4_000;
        assert_eq!(record.last_used_ms(Some(&status)), 4_000);

        status.state = AgentState::Working;
        status.state_since_ms = 9_000;
        status.turn_started_ms = Some(9_000);
        assert_eq!(record.last_used_ms(Some(&status)), 9_000, "a turn that began on its own counts too");

        status.state = AgentState::Done;
        status.state_since_ms = 15_000;
        status.turn_started_ms = None;
        status.last_turn_ms = Some(6_000);
        assert_eq!(status.last_turn_started_ms(), Some(9_000));
        assert_eq!(record.last_used_ms(Some(&status)), 9_000, "finishing the turn does not move it again");

        status.input_at_ms = 20_000;
        assert_eq!(record.last_used_ms(Some(&status)), 20_000);
    }

    #[test]
    fn a_terminal_is_used_when_typed_into_not_when_it_prints() {
        let record: AgentRecord = serde_json::from_str(
            r#"{"id":2,"session":"s","name":"dev","kind":"terminal","cwd":"/x","created_at_ms":1000}"#,
        )
        .unwrap();
        let mut status: StatusReport = serde_json::from_str(
            r#"{"state":"working","state_since_ms":8000,"turn_started_ms":8000,"started_at_ms":1000,"rows":24,"cols":80}"#,
        )
        .unwrap();
        assert_eq!(record.last_used_ms(Some(&status)), 1_000, "a dev server printing logs");
        status.input_at_ms = 3_000;
        assert_eq!(record.last_used_ms(Some(&status)), 3_000);
        status.nested =
            Some(NestedAgent { kind: AgentKind::Claude, pid: 7, conversation_id: None, cwd: None, since_ms: 2_000 });
        assert_eq!(record.last_used_ms(Some(&status)), 8_000, "claude running inside the terminal");
    }

    #[test]
    fn agents_in_a_worktree_belong_to_the_folder_they_were_started_from() {
        let mut record: AgentRecord = serde_json::from_str(
            r#"{"id":1,"session":"s","name":"n","kind":"claude","cwd":"/repo/app","created_at_ms":1}"#,
        )
        .unwrap();
        assert_eq!(record.worktree, None);
        assert_eq!(record.project_dir(), Path::new("/repo/app"));
        record.cwd = PathBuf::from("/orkai/worktrees/1/app");
        record.worktree = Some(Worktree {
            repo: PathBuf::from("/repo"),
            path: PathBuf::from("/orkai/worktrees/1"),
            branch: "orkai/n".into(),
            base: "abc".into(),
            source: PathBuf::from("/repo/app"),
        });
        assert_eq!(record.project_dir(), Path::new("/repo/app"));
        let saved = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<AgentRecord>(&saved).unwrap(), record);
    }

    #[test]
    fn agents_working_inside_another_agents_worktree_belong_to_its_project() {
        let mut record: AgentRecord = serde_json::from_str(
            r#"{"id":2,"session":"s","name":"review","kind":"codex","cwd":"/orkai/worktrees/1/app","created_at_ms":1}"#,
        )
        .unwrap();
        assert_eq!(record.project, None);
        assert_eq!(record.project_dir(), Path::new("/orkai/worktrees/1/app"));
        record.project = Some(PathBuf::from("/repo/app"));
        assert_eq!(record.project_dir(), Path::new("/repo/app"));
        let saved = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<AgentRecord>(&saved).unwrap(), record);
    }

    #[test]
    fn exit_info_describes_outcome() {
        let ok = ExitInfo { code: Some(0), error: None, at_ms: 1 };
        let failed = ExitInfo { code: None, error: Some("not found".into()), at_ms: 1 };
        assert_eq!(ok.describe(), "exited with code 0");
        assert_eq!(failed.describe(), "failed: not found");
    }
}
