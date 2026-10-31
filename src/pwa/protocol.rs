use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{AgentId, AgentKind, AgentState, TestState};

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
    Open {
        agent: AgentId,
        rows: u16,
        cols: u16,
    },
    Close,
    Size {
        rows: u16,
        cols: u16,
    },
    Keys {
        data: String,
    },
    Send {
        text: String,
    },
    Paste {
        text: String,
    },
    Key {
        key: String,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        shift: bool,
    },
    Scroll {
        lines: i32,
    },
    Stop {
        agent: AgentId,
    },
    Remove {
        agent: AgentId,
    },
    Seen {
        agent: AgentId,
    },
    Rename {
        agent: AgentId,
        name: String,
    },
    Start(StartRequest),
    Session {
        name: String,
    },
    Recover {
        session: String,
    },
    Folders {
        path: String,
    },
    Diff {
        agent: AgentId,
    },
    Merge {
        agent: AgentId,
    },
    Peek {
        agents: Vec<AgentId>,
        rows: u16,
    },
    Details {
        agent: AgentId,
    },
    Restart {
        agent: AgentId,
    },
    CrossReview {
        agent: AgentId,
        #[serde(default)]
        reviewer: String,
    },
    Tests {
        agent: AgentId,
    },
    LinkAt {
        row: u16,
        col: u16,
    },
    Skills {
        kind: AgentKind,
        cwd: String,
    },
    Skill {
        kind: AgentKind,
        session: String,
        cwd: String,
        name: String,
        #[serde(default)]
        args: String,
        #[serde(default)]
        worktree: bool,
        #[serde(default)]
        bypass: bool,
    },
    Race {
        session: String,
        cwd: String,
        race: String,
        #[serde(default)]
        bypass: bool,
    },
    Compare {
        agent: AgentId,
    },
    Pick {
        agent: AgentId,
    },
    RenameSession {
        name: String,
        to: String,
    },
    DeleteSession {
        name: String,
    },
    Trust {
        cwd: String,
        #[serde(default)]
        command: Option<String>,
    },
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
    Peek { previews: Vec<Preview> },
    Details(Details),
    Tests { agent: AgentId, command: String, state: TestState, text: String, truncated: bool },
    Link { url: String },
    Skills { kind: AgentKind, cwd: String, skills: Vec<SkillView> },
    Compare { agent: AgentId, title: String, text: String },
    TrustAsk { cwd: String, repo: String, command: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillView {
    pub name: String,
    pub owner: AgentKind,
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

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub agent: AgentId,
    pub subagents: Vec<SubagentView>,
    pub changes: Option<Vec<FileView>>,
    pub overlaps: Vec<OverlapView>,
    pub test: Option<TestView>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentView {
    pub role: String,
    pub description: String,
    pub activity: String,
    pub started_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlapView {
    pub agent: AgentId,
    pub name: String,
    pub paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestView {
    pub state: TestState,
    pub command: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub code: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub agent: AgentId,
    pub lines: Vec<Vec<Run>>,
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
            (r#"{"type":"peek","agents":[2,5],"rows":6}"#, PhoneMsg::Peek { agents: vec![2, 5], rows: 6 }),
            (r#"{"type":"details","agent":4}"#, PhoneMsg::Details { agent: 4 }),
            (
                r#"{"type":"race","session":"s","cwd":"/w","race":"cc,codex fix"}"#,
                PhoneMsg::Race { session: "s".into(), cwd: "/w".into(), race: "cc,codex fix".into(), bypass: false },
            ),
            (r#"{"type":"trust","cwd":"/w"}"#, PhoneMsg::Trust { cwd: "/w".into(), command: None }),
            (
                r#"{"type":"renameSession","name":"a","to":"b"}"#,
                PhoneMsg::RenameSession { name: "a".into(), to: "b".into() },
            ),
            (r#"{"type":"crossReview","agent":4}"#, PhoneMsg::CrossReview { agent: 4, reviewer: String::new() }),
            (
                r#"{"type":"crossReview","agent":4,"reviewer":"cc"}"#,
                PhoneMsg::CrossReview { agent: 4, reviewer: "cc".into() },
            ),
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

    #[test]
    fn previews_carry_the_last_lines_of_each_agent() {
        let peek = BridgeMsg::Peek { previews: vec![Preview { agent: 3, lines: vec![vec![("ok".into(), 2, -1, 0)]] }] };
        let text = serde_json::to_string(&peek).unwrap();
        assert_eq!(text, r#"{"type":"peek","previews":[{"agent":3,"lines":[[["ok",2,-1,0]]]}]}"#);
    }
}
