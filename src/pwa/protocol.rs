use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{AgentId, AgentKind, AgentState};

pub const VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Greeting {
    Hello { version: u32, key: String },
    Refused { reason: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PhoneMsg {
    Sync,
    Open { agent: AgentId, rows: u16, cols: u16 },
    Close,
    Size { rows: u16, cols: u16 },
    Keys { data: String },
    Send { text: String },
    Scroll { lines: i32 },
    Stop { agent: AgentId },
    Remove { agent: AgentId },
    Seen { agent: AgentId },
    Rename { agent: AgentId, name: String },
    Start(StartRequest),
    Session { name: String },
    Recover { session: String },
    Folders { path: String },
    Diff { agent: AgentId },
    Merge { agent: AgentId },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub kind: AgentKind,
    pub session: String,
    pub cwd: String,
    pub task: String,
    pub worktree: bool,
    #[serde(default)]
    pub bypass: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BridgeMsg {
    Welcome { host: String, version: String },
    State(Overview),
    Usage { entries: Vec<UsageEntry> },
    Screen(Screen),
    Notice { text: String, tone: Tone },
    Folders { path: String, parent: Option<String>, dirs: Vec<Dir> },
    Diff { agent: AgentId, branch: String, text: String, truncated: bool },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub host: String,
    pub now: u64,
    pub session: String,
    pub sessions: Vec<SessionView>,
    pub agents: Vec<AgentView>,
    pub where_dir: String,
    pub dirs: BTreeMap<String, String>,
    pub worktree: bool,
    pub kinds: Vec<AgentKind>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub name: String,
    pub agents: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub id: AgentId,
    pub session: String,
    pub name: String,
    pub kind: AgentKind,
    pub state: AgentState,
    pub starting: bool,
    pub activity: String,
    pub task: String,
    pub cwd: String,
    pub project: String,
    pub color: String,
    pub started_at: Option<u64>,
    pub state_since: Option<u64>,
    pub turn_started: Option<u64>,
    pub last_turn: Option<u64>,
    pub branch: Option<String>,
    pub exit: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub kind: AgentKind,
    pub windows: Vec<UsageWindow>,
    pub credits: Option<f64>,
    pub problem: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub label: String,
    pub used: f64,
    pub resets_at: Option<u64>,
}

pub type Run = (String, i32, i32, u8);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    pub agent: AgentId,
    pub rows: u16,
    pub cols: u16,
    pub full: bool,
    pub cursor: Option<(u16, u16)>,
    pub scroll: usize,
    pub lines: Vec<(u16, Vec<Run>)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Dir {
    pub name: String,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_greeting_is_plain_camel_case_json() {
        let hello = Greeting::Hello { version: VERSION, key: "pk".into() };
        assert_eq!(serde_json::to_string(&hello).unwrap(), r#"{"type":"hello","version":2,"key":"pk"}"#);
    }

    #[test]
    fn phone_messages_read_the_way_the_app_writes_them() {
        let cases = [
            (r#"{"type":"sync"}"#, PhoneMsg::Sync),
            (r#"{"type":"open","agent":3,"rows":30,"cols":48}"#, PhoneMsg::Open { agent: 3, rows: 30, cols: 48 }),
            (r#"{"type":"keys","data":"\u001b[A"}"#, PhoneMsg::Keys { data: "\x1b[A".into() }),
            (r#"{"type":"send","text":"fix it"}"#, PhoneMsg::Send { text: "fix it".into() }),
            (r#"{"type":"scroll","lines":-3}"#, PhoneMsg::Scroll { lines: -3 }),
            (
                r#"{"type":"start","kind":"claude","session":"default","cwd":"/w","task":"t","worktree":true}"#,
                PhoneMsg::Start(StartRequest {
                    kind: AgentKind::Claude,
                    session: "default".into(),
                    cwd: "/w".into(),
                    task: "t".into(),
                    worktree: true,
                    bypass: false,
                }),
            ),
            (
                r#"{"type":"start","kind":"codex","session":"s","cwd":"~","task":"","worktree":false,"bypass":true}"#,
                PhoneMsg::Start(StartRequest {
                    kind: AgentKind::Codex,
                    session: "s".into(),
                    cwd: "~".into(),
                    task: String::new(),
                    worktree: false,
                    bypass: true,
                }),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(serde_json::from_str::<PhoneMsg>(text).unwrap(), expected, "{text}");
        }
    }

    #[test]
    fn an_unknown_phone_message_is_an_error() {
        assert!(serde_json::from_str::<PhoneMsg>(r#"{"type":"format","disk":"c"}"#).is_err());
    }

    #[test]
    fn the_overview_flattens_into_the_state_message() {
        let state = BridgeMsg::State(Overview { host: "pc".into(), where_dir: "/w".into(), ..Overview::default() });
        let value: serde_json::Value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["type"], "state");
        assert_eq!(value["host"], "pc");
        assert_eq!(value["whereDir"], "/w");
    }

    #[test]
    fn screen_runs_are_compact_arrays() {
        let screen = BridgeMsg::Screen(Screen {
            agent: 1,
            rows: 2,
            cols: 10,
            full: true,
            cursor: Some((1, 4)),
            scroll: 0,
            lines: vec![(0, vec![("hi".into(), -1, 4, 1)])],
        });
        let text = serde_json::to_string(&screen).unwrap();
        assert!(text.contains(r#""lines":[[0,[["hi",-1,4,1]]]]"#), "{text}");
        assert!(text.contains(r#""cursor":[1,4]"#), "{text}");
    }
}
