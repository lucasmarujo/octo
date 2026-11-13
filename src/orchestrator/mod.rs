use serde::{Deserialize, Serialize};

use crate::model::{AgentId, AgentKind, AgentState};

pub mod engine;
pub mod instructions;
pub mod logs;
pub mod prompt;
pub mod question;
pub mod relay;
pub mod runner;
pub mod settings;
pub mod verdict;

pub use engine::{CliEngine, Engine};
pub use runner::Orchestrator;
pub use settings::{ProjectChange, Settings, SettingsChange};

pub const ENGINES: [AgentKind; 2] = [AgentKind::Claude, AgentKind::Codex];
pub const MAX_ENGINE_PROCESSES: usize = 2;
pub const MAX_DECISIONS: usize = 256;
pub const MAX_ACTIVITY: usize = 50;
pub const MAX_CHAT: usize = 60;
pub const MAX_CONTEXT_ROWS: usize = 40;
pub const MAX_ROW_CHARS: usize = 200;
pub const MAX_QUESTION_CHARS: usize = 240;
pub const MAX_KEYS: usize = 16;
pub const MAX_ANSWER_CHARS: usize = 4_000;
pub const ACTIVITY_PAGE: usize = 50;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Observe,
    Moderate,
    Bypass,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DecisionStatus {
    Deciding,
    NeedsYou,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum By {
    Octo,
    Rule,
    You,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Answered,
    Denied,
    Escalated,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum How {
    Suggestion,
    Text,
    Keys,
    Allow,
    Dismiss,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    You,
    Octo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Keys(Vec<String>),
    Text(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub keys: Option<Vec<String>>,
    pub text: Option<String>,
    pub summary: String,
    pub reason: String,
}

impl Suggestion {
    pub fn of(input: Input, summary: String, reason: String) -> Self {
        match input {
            Input::Keys(keys) => Self { keys: Some(keys), text: None, summary, reason },
            Input::Text(text) => Self { keys: None, text: Some(text), summary, reason },
        }
    }

    pub fn input(&self) -> Option<Input> {
        match (&self.keys, &self.text) {
            (Some(keys), _) => Some(Input::Keys(keys.clone())),
            (None, Some(text)) => Some(Input::Text(text.clone())),
            (None, None) => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolAsk {
    pub name: String,
    pub detail: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub id: u64,
    pub agent: AgentId,
    pub name: String,
    pub kind: AgentKind,
    pub session: String,
    pub project: String,
    pub since: u64,
    pub status: DecisionStatus,
    pub question: String,
    pub context: Vec<String>,
    pub tool: Option<ToolAsk>,
    pub suggestion: Option<Suggestion>,
    pub tag: Option<String>,
    pub problem: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub seq: u64,
    pub at: u64,
    pub agent: AgentId,
    pub name: String,
    pub kind: AgentKind,
    pub project: String,
    pub question: String,
    pub summary: String,
    pub by: By,
    pub outcome: Outcome,
    pub mode: Mode,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatEntry {
    pub at: u64,
    pub role: ChatRole,
    pub text: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub failed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMode {
    pub path: String,
    pub mode: Mode,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCount {
    pub path: String,
    pub count: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineView {
    pub kind: AgentKind,
    pub installed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorView {
    #[serde(default)]
    pub online: bool,
    pub enabled: bool,
    pub mode: Mode,
    pub projects: Vec<ProjectMode>,
    pub primary: Option<AgentKind>,
    pub secondary: Option<AgentKind>,
    pub primary_model: Option<String>,
    pub secondary_model: Option<String>,
    pub engines: Vec<EngineView>,
    pub decisions: Vec<Decision>,
    pub handled: Vec<ActivityEntry>,
    pub handled_today: u32,
    pub handled_by_project: Vec<ProjectCount>,
    pub chat: Vec<ChatEntry>,
    pub thinking: bool,
    pub problem: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserAnswer {
    pub decision: u64,
    pub how: How,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub keys: Option<Vec<String>>,
    #[serde(default)]
    pub remember: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sight {
    Unknown,
    Paused(u64),
    Other,
}

impl Sight {
    pub fn of(state: AgentState, since: u64) -> Self {
        if state == AgentState::Paused { Sight::Paused(since) } else { Sight::Other }
    }
}

pub type Sighting = (AgentId, Sight);

pub fn cut(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect()
}

pub fn one_line(text: &str, chars: usize) -> String {
    cut(&text.split_whitespace().collect::<Vec<_>>().join(" "), chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_and_labels_travel_as_the_words_the_window_uses() {
        let words = |value: serde_json::Value| value.as_str().map(str::to_string).unwrap();
        assert_eq!(
            [Mode::Observe, Mode::Moderate, Mode::Bypass].map(|mode| words(serde_json::to_value(mode).unwrap())),
            ["observe", "moderate", "bypass"]
        );
        assert_eq!(Mode::default(), Mode::Observe, "Octo never answers until it is told to");
        assert_eq!(serde_json::to_value(DecisionStatus::NeedsYou).unwrap(), "needsYou");
        assert_eq!(serde_json::to_value(DecisionStatus::Deciding).unwrap(), "deciding");
        assert_eq!(
            serde_json::to_value([By::Octo, By::Rule, By::You]).unwrap(),
            serde_json::json!(["octo", "rule", "you"])
        );
        assert_eq!(
            serde_json::to_value([Outcome::Answered, Outcome::Denied, Outcome::Escalated, Outcome::Failed]).unwrap(),
            serde_json::json!(["answered", "denied", "escalated", "failed"])
        );
        let hows: Vec<How> = serde_json::from_str(r#"["suggestion","text","keys","allow","dismiss"]"#).unwrap();
        assert_eq!(hows, [How::Suggestion, How::Text, How::Keys, How::Allow, How::Dismiss]);
        assert!(serde_json::from_str::<Mode>(r#""yolo""#).is_err());
    }

    #[test]
    fn a_suggestion_carries_exactly_one_of_keys_or_text() {
        let keys = Suggestion::of(Input::Keys(vec!["1".into()]), "Allowed it".into(), "safe".into());
        assert_eq!(
            serde_json::to_value(&keys).unwrap(),
            serde_json::json!({ "keys": ["1"], "text": null, "summary": "Allowed it", "reason": "safe" })
        );
        assert_eq!(keys.input(), Some(Input::Keys(vec!["1".into()])));
        let text = Suggestion::of(Input::Text("keep it".into()), String::new(), String::new());
        assert_eq!((text.keys.clone(), text.text.as_deref()), (None, Some("keep it")));
        assert_eq!(text.input(), Some(Input::Text("keep it".into())));
        let empty = Suggestion { keys: None, text: None, summary: String::new(), reason: String::new() };
        assert_eq!(empty.input(), None);
    }

    #[test]
    fn the_view_goes_out_in_camel_case_with_every_field_the_window_reads() {
        let view = OrchestratorView {
            primary: Some(AgentKind::Claude),
            primary_model: Some("opus".into()),
            engines: vec![EngineView { kind: AgentKind::Codex, installed: false }],
            handled_today: 3,
            handled_by_project: vec![ProjectCount { path: "dev/octo".into(), count: 3 }],
            ..OrchestratorView::default()
        };
        let value = serde_json::to_value(&view).unwrap();
        assert_eq!(value["online"], false);
        let mut older = value.clone();
        older.as_object_mut().unwrap().remove("online");
        assert_eq!(serde_json::from_value::<OrchestratorView>(older).unwrap(), view, "a view without it still reads");
        assert_eq!(value["enabled"], false);
        assert_eq!(value["mode"], "observe");
        assert_eq!(value["primary"], "claude");
        assert_eq!(value["secondary"], serde_json::Value::Null);
        assert_eq!(value["primaryModel"], "opus");
        assert_eq!(value["secondaryModel"], serde_json::Value::Null);
        assert_eq!(value["engines"], serde_json::json!([{ "kind": "codex", "installed": false }]));
        assert_eq!(value["handledToday"], 3);
        assert_eq!(value["handledByProject"], serde_json::json!([{ "path": "dev/octo", "count": 3 }]));
        for field in ["projects", "decisions", "handled", "chat"] {
            assert_eq!(value[field], serde_json::json!([]), "{field}");
        }
        assert_eq!(value["thinking"], false);
        assert_eq!(value["problem"], serde_json::Value::Null);
    }

    #[test]
    fn a_decision_and_its_log_entries_go_out_in_camel_case() {
        let decision = Decision {
            id: 4,
            agent: 2,
            name: "api".into(),
            kind: AgentKind::Claude,
            session: "default".into(),
            project: "dev/octo".into(),
            since: 9,
            status: DecisionStatus::NeedsYou,
            question: "Run npm test?".into(),
            context: vec!["Run npm test?".into()],
            tool: Some(ToolAsk { name: "Bash".into(), detail: "npm test".into() }),
            suggestion: None,
            tag: Some("no rule yet".into()),
            problem: None,
        };
        assert_eq!(
            serde_json::to_string(&decision).unwrap(),
            r#"{"id":4,"agent":2,"name":"api","kind":"claude","session":"default","project":"dev/octo","since":9,"status":"needsYou","question":"Run npm test?","context":["Run npm test?"],"tool":{"name":"Bash","detail":"npm test"},"suggestion":null,"tag":"no rule yet","problem":null}"#
        );
        let entry = ActivityEntry {
            seq: 1,
            at: 2,
            agent: 3,
            name: "api".into(),
            kind: AgentKind::Codex,
            project: "dev/octo".into(),
            question: "Run it?".into(),
            summary: "Allowed npm test".into(),
            by: By::Rule,
            outcome: Outcome::Answered,
            mode: Mode::Bypass,
        };
        assert_eq!(
            serde_json::to_string(&entry).unwrap(),
            r#"{"seq":1,"at":2,"agent":3,"name":"api","kind":"codex","project":"dev/octo","question":"Run it?","summary":"Allowed npm test","by":"rule","outcome":"answered","mode":"bypass"}"#
        );
        let said = ChatEntry {
            at: 5,
            role: ChatRole::Octo,
            text: "done".into(),
            actions: vec!["Stopping api".into()],
            failed: false,
        };
        assert_eq!(
            serde_json::to_string(&said).unwrap(),
            r#"{"at":5,"role":"octo","text":"done","actions":["Stopping api"],"failed":false}"#
        );
    }

    #[test]
    fn an_agent_is_seen_paused_only_in_the_paused_state() {
        assert_eq!(Sight::of(AgentState::Paused, 7), Sight::Paused(7));
        for state in [AgentState::Idle, AgentState::Working, AgentState::Done, AgentState::Exited, AgentState::Lost] {
            assert_eq!(Sight::of(state, 7), Sight::Other, "{state:?}");
        }
    }

    #[test]
    fn long_text_is_cut_by_characters_and_flattened_to_one_line() {
        assert_eq!(cut("ação", 2), "aç");
        assert_eq!(cut("ok", 9), "ok");
        assert_eq!(one_line("  run\n the\ttests  ", 40), "run the tests");
        assert_eq!(one_line("one two three", 7), "one two");
    }
}
