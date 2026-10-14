use std::path::PathBuf;

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
    Terminal,
}

impl AgentKind {
    pub const ALL: [AgentKind; 3] = [AgentKind::Claude, AgentKind::Codex, AgentKind::Terminal];

    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Terminal => "terminal",
        }
    }

    pub fn is_ai(self) -> bool {
        !matches!(self, AgentKind::Terminal)
    }

    pub fn next(self) -> Self {
        match self {
            AgentKind::Claude => AgentKind::Codex,
            AgentKind::Codex => AgentKind::Terminal,
            AgentKind::Terminal => AgentKind::Claude,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            AgentKind::Claude => AgentKind::Terminal,
            AgentKind::Codex => AgentKind::Claude,
            AgentKind::Terminal => AgentKind::Codex,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "claude" | "cc" => Some(AgentKind::Claude),
            "codex" => Some(AgentKind::Codex),
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
}

impl Default for UiState {
    fn default() -> Self {
        Self { session: DEFAULT_SESSION.to_string(), who: AgentKind::Claude, where_dir: None, tabs: Vec::new() }
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
        for _ in 0..3 {
            kind = kind.next();
        }
        assert_eq!(kind, AgentKind::Claude);
        assert_eq!(AgentKind::Claude.prev(), AgentKind::Terminal);
        assert_eq!(AgentKind::Terminal.next(), AgentKind::Claude);
    }

    #[test]
    fn kind_parses_aliases() {
        assert_eq!(AgentKind::parse("Claude"), Some(AgentKind::Claude));
        assert_eq!(AgentKind::parse("shell"), Some(AgentKind::Terminal));
        assert_eq!(AgentKind::parse("codex"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::parse("vim"), None);
    }

    #[test]
    fn kind_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&AgentKind::Terminal).unwrap(), "\"terminal\"");
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
    }

    #[test]
    fn exit_info_describes_outcome() {
        let ok = ExitInfo { code: Some(0), error: None, at_ms: 1 };
        let failed = ExitInfo { code: None, error: Some("not found".into()), at_ms: 1 };
        assert_eq!(ok.describe(), "exited with code 0");
        assert_eq!(failed.describe(), "failed: not found");
    }
}
