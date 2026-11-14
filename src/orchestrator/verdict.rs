use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::model::AgentId;
use crate::orchestrator::{Input, MAX_ANSWER_CHARS, MAX_KEYS, Mode, Suggestion, cut, one_line};
use crate::tui::keys;

pub const MAX_SUMMARY_CHARS: usize = 160;
pub const MAX_REASON_CHARS: usize = 300;
pub const MAX_TAG_CHARS: usize = 40;
pub const MAX_REPLY_CHARS: usize = 8_000;
pub const MAX_READ_AGENTS: usize = 4;
pub const MAX_ACTIONS: usize = 8;
const OPEN: u8 = b'{';
const CLOSE: u8 = b'}';
const QUOTE: u8 = b'"';
const ESCAPE: u8 = b'\\';
const UNDECIDED: &str = "Octo could not decide";
const UNUSABLE: &str = "Octo's answer could not be typed into the agent";

fn balanced(text: &str) -> Option<&str> {
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for (at, byte) in text.bytes().enumerate() {
        match byte {
            _ if escaped => escaped = false,
            ESCAPE if quoted => escaped = true,
            QUOTE => quoted = !quoted,
            OPEN if !quoted => depth += 1,
            CLOSE if !quoted => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&text[..=at]);
                }
            }
            _ => {}
        }
    }
    None
}

pub fn first_object<T: DeserializeOwned>(text: &str) -> Option<T> {
    text.match_indices(char::from(OPEN))
        .filter_map(|(at, _)| balanced(&text[at..]))
        .find_map(|object| serde_json::from_str(object).ok())
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Answer,
    Escalate,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub action: Action,
    #[serde(default)]
    pub keys: Option<Vec<String>>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub deny: bool,
}

pub fn encode_keys(names: &[String], app_cursor: bool) -> Option<Vec<Vec<u8>>> {
    if names.is_empty() || names.len() > MAX_KEYS {
        return None;
    }
    names
        .iter()
        .map(|name| keys::dom_key(name, false, false, false).and_then(|key| keys::encode_key(&key, app_cursor)))
        .collect()
}

pub fn usable_input(keys: Option<Vec<String>>, text: Option<String>) -> Option<Input> {
    let keys = keys.filter(|keys| !keys.is_empty());
    let text = text.map(|text| cut(text.trim(), MAX_ANSWER_CHARS)).filter(|text| !text.is_empty());
    match (keys, text) {
        (Some(keys), _) => encode_keys(&keys, false).map(|_| Input::Keys(keys)),
        (None, Some(text)) => Some(Input::Text(text)),
        (None, None) => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ruling {
    Apply { suggestion: Suggestion, deny: bool },
    NeedsYou { suggestion: Option<Suggestion>, tag: Option<String>, problem: Option<String> },
}

pub fn rule(mode: Mode, verdict: Verdict) -> Ruling {
    let summary = one_line(&verdict.summary, MAX_SUMMARY_CHARS);
    let reason = one_line(&verdict.reason, MAX_REASON_CHARS);
    let tag = verdict.tag.map(|tag| one_line(&tag, MAX_TAG_CHARS)).filter(|tag| !tag.is_empty());
    let undecided = if reason.is_empty() { UNDECIDED.to_string() } else { format!("{UNDECIDED}: {reason}") };
    let suggestion = usable_input(verdict.keys, verdict.text).map(|input| Suggestion::of(input, summary, reason));
    match (mode, verdict.action, suggestion) {
        (Mode::Observe, _, suggestion) => Ruling::NeedsYou { suggestion, tag, problem: None },
        (Mode::Moderate, Action::Escalate, suggestion) => Ruling::NeedsYou { suggestion, tag, problem: None },
        (Mode::Bypass, Action::Escalate, suggestion) => Ruling::NeedsYou { suggestion, tag, problem: Some(undecided) },
        (Mode::Moderate | Mode::Bypass, Action::Answer, Some(suggestion)) => {
            Ruling::Apply { suggestion, deny: verdict.deny }
        }
        (Mode::Moderate | Mode::Bypass, Action::Answer, None) => {
            Ruling::NeedsYou { suggestion: None, tag, problem: Some(UNUSABLE.to_string()) }
        }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct ChatAnswer {
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub read: Option<Vec<AgentId>>,
    #[serde(default)]
    pub actions: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ChatAction {
    Send {
        agent: AgentId,
        text: String,
    },
    Answer {
        decision: u64,
        #[serde(default)]
        keys: Option<Vec<String>>,
        #[serde(default)]
        text: Option<String>,
    },
    Stop {
        agent: AgentId,
    },
    Start {
        kind: String,
        cwd: String,
        #[serde(default)]
        task: String,
        #[serde(default)]
        session: Option<String>,
        #[serde(default)]
        model: Option<String>,
    },
    Restart {
        agent: AgentId,
    },
    Remove {
        agent: AgentId,
    },
    Rename {
        agent: AgentId,
        name: String,
    },
}

impl ChatAnswer {
    pub fn reads(&self) -> Vec<AgentId> {
        let mut agents: Vec<AgentId> = Vec::new();
        for agent in self.read.iter().flatten() {
            if !agents.contains(agent) && agents.len() < MAX_READ_AGENTS {
                agents.push(*agent);
            }
        }
        agents
    }

    pub fn actions(&self) -> Vec<Option<ChatAction>> {
        self.actions
            .iter()
            .flatten()
            .take(MAX_ACTIONS)
            .map(|action| serde_json::from_value(action.clone()).ok())
            .collect()
    }

    pub fn said(&self) -> String {
        cut(self.reply.trim(), MAX_REPLY_CHARS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(text: &str) -> Option<Verdict> {
        first_object(text)
    }

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    const ANSWER: &str = r#"{"action":"answer","keys":["1"],"text":null,"summary":"Allowed npm test","reason":"the task asks for it","tag":null,"deny":false}"#;

    #[test]
    fn the_answer_is_the_first_whole_object_whatever_surrounds_it() {
        let plain = verdict(ANSWER).unwrap();
        assert_eq!(plain.action, Action::Answer);
        assert_eq!(plain.keys, Some(keys(&["1"])));
        assert_eq!((plain.summary.as_str(), plain.reason.as_str()), ("Allowed npm test", "the task asks for it"));
        assert_eq!((plain.text.clone(), plain.tag.clone(), plain.deny), (None, None, false));
        assert_eq!(verdict(&format!("```json\n{ANSWER}\n```")), Some(plain.clone()), "models wrap it in fences");
        assert_eq!(verdict(&format!("Here is my answer:\n\n{ANSWER}\n\nHope it helps {{}}")), Some(plain.clone()));
        assert_eq!(
            verdict(&format!("I would use {{braces}} here. {ANSWER}")),
            Some(plain.clone()),
            "not every brace is it"
        );
        assert_eq!(verdict(&format!("{ANSWER}{ANSWER}")), Some(plain));
    }

    #[test]
    fn braces_inside_strings_and_nested_objects_do_not_end_the_object() {
        let tricky =
            r#"{"action":"escalate","text":"use {a} and \"}\" and \\","summary":"s","extra":{"deep":{"x":1}}}"#;
        let read = verdict(tricky).unwrap();
        assert_eq!(read.action, Action::Escalate);
        assert_eq!(read.text.as_deref(), Some(r#"use {a} and "}" and \"#));
        assert_eq!(read.reason, "", "a field left out is empty");
    }

    #[test]
    fn a_cut_or_malformed_answer_is_no_answer() {
        assert_eq!(verdict(""), None);
        assert_eq!(verdict("I cannot answer that."), None);
        assert_eq!(verdict(&ANSWER[..ANSWER.len() - 1]), None, "an object cut short is not guessed");
        assert_eq!(verdict(r#"{"action":"answer","keys":["1"]"#), None);
        assert_eq!(verdict(r#"{"action":"approve","keys":["1"]}"#), None, "an unknown action is not an answer");
        assert_eq!(verdict(r#"{"keys":["1"]}"#), None, "the action is required");
        assert_eq!(verdict(r#"{"action":"answer","keys":"1"}"#), None);
        assert_eq!(verdict("}{"), None);
        assert_eq!(verdict(r#"{"action":"answer" "#), None);
    }

    #[test]
    fn keys_are_the_names_the_hub_already_accepts() {
        assert_eq!(encode_keys(&keys(&["ArrowDown", "Enter"]), false), Some(vec![b"\x1b[B".to_vec(), b"\r".to_vec()]));
        assert_eq!(encode_keys(&keys(&["ArrowDown"]), true), Some(vec![b"\x1bOB".to_vec()]));
        assert_eq!(encode_keys(&keys(&["y"]), false), Some(vec![b"y".to_vec()]));
        assert_eq!(encode_keys(&keys(&["Escape", "Tab", "2"]), false).map(|keys| keys.len()), Some(3));
        assert_eq!(encode_keys(&keys(&["Enter", "Bogus"]), false), None, "one unknown key spoils the answer");
        assert_eq!(encode_keys(&[], false), None);
        assert_eq!(encode_keys(&vec!["1".to_string(); MAX_KEYS + 1], false), None);
        assert!(encode_keys(&vec!["1".to_string(); MAX_KEYS], false).is_some());
    }

    #[test]
    fn an_answer_is_keys_or_text_and_never_both() {
        assert_eq!(usable_input(Some(keys(&["1"])), None), Some(Input::Keys(keys(&["1"]))));
        assert_eq!(usable_input(None, Some("  keep it  ".into())), Some(Input::Text("keep it".into())));
        assert_eq!(usable_input(Some(keys(&["1"])), Some("yes".into())), Some(Input::Keys(keys(&["1"]))), "keys win");
        assert_eq!(usable_input(Some(Vec::new()), Some("yes".into())), Some(Input::Text("yes".into())));
        assert_eq!(usable_input(Some(keys(&["Bogus"])), Some("yes".into())), None, "bad keys are not swapped for text");
        assert_eq!(usable_input(None, Some("   ".into())), None);
        assert_eq!(usable_input(None, None), None);
        let long = usable_input(None, Some("x".repeat(MAX_ANSWER_CHARS + 9))).unwrap();
        assert_eq!(long, Input::Text("x".repeat(MAX_ANSWER_CHARS)));
    }

    fn judged(action: Action, keys: Option<Vec<String>>, text: Option<&str>) -> Verdict {
        Verdict {
            action,
            keys,
            text: text.map(str::to_string),
            summary: "Allowed  npm test".into(),
            reason: "the task\nasks for it".into(),
            tag: Some(" no rule yet ".into()),
            deny: false,
        }
    }

    fn suggested(input: Input) -> Suggestion {
        Suggestion::of(input, "Allowed npm test".into(), "the task asks for it".into())
    }

    #[test]
    fn observe_never_applies_what_octo_would_answer() {
        let one = Input::Keys(keys(&["1"]));
        for action in [Action::Answer, Action::Escalate] {
            assert_eq!(
                rule(Mode::Observe, judged(action, Some(keys(&["1"])), None)),
                Ruling::NeedsYou {
                    suggestion: Some(suggested(one.clone())),
                    tag: Some("no rule yet".into()),
                    problem: None
                },
                "{action:?}"
            );
        }
        assert_eq!(
            rule(Mode::Observe, judged(Action::Escalate, None, None)),
            Ruling::NeedsYou { suggestion: None, tag: Some("no rule yet".into()), problem: None }
        );
    }

    #[test]
    fn moderate_applies_an_answer_and_leaves_an_escalation_with_its_suggestion() {
        let typed = Input::Text("keep the old endpoint".into());
        assert_eq!(
            rule(Mode::Moderate, judged(Action::Answer, None, Some("keep the old endpoint"))),
            Ruling::Apply { suggestion: suggested(typed.clone()), deny: false }
        );
        let denied = Verdict { deny: true, ..judged(Action::Answer, Some(keys(&["Escape"])), None) };
        assert_eq!(
            rule(Mode::Moderate, denied),
            Ruling::Apply { suggestion: suggested(Input::Keys(keys(&["Escape"]))), deny: true }
        );
        assert_eq!(
            rule(Mode::Moderate, judged(Action::Escalate, None, Some("keep the old endpoint"))),
            Ruling::NeedsYou { suggestion: Some(suggested(typed)), tag: Some("no rule yet".into()), problem: None }
        );
        assert_eq!(
            rule(Mode::Moderate, judged(Action::Escalate, None, None)),
            Ruling::NeedsYou { suggestion: None, tag: Some("no rule yet".into()), problem: None }
        );
    }

    #[test]
    fn bypass_applies_an_answer_and_an_escalation_waits_with_the_problem() {
        let one = Input::Keys(keys(&["1"]));
        assert_eq!(
            rule(Mode::Bypass, judged(Action::Answer, Some(keys(&["1"])), None)),
            Ruling::Apply { suggestion: suggested(one.clone()), deny: false }
        );
        assert_eq!(
            rule(Mode::Bypass, judged(Action::Escalate, Some(keys(&["1"])), None)),
            Ruling::NeedsYou {
                suggestion: Some(suggested(one)),
                tag: Some("no rule yet".into()),
                problem: Some("Octo could not decide: the task asks for it".into()),
            }
        );
        let silent = Verdict { reason: String::new(), tag: None, ..judged(Action::Escalate, None, None) };
        assert_eq!(
            rule(Mode::Bypass, silent),
            Ruling::NeedsYou { suggestion: None, tag: None, problem: Some("Octo could not decide".into()) }
        );
    }

    #[test]
    fn an_answer_that_cannot_be_typed_is_left_for_the_user_in_every_mode_that_applies() {
        for mode in [Mode::Moderate, Mode::Bypass] {
            for (keys, text) in [(None, None), (Some(keys(&["Bogus"])), None), (None, Some("  "))] {
                let Ruling::NeedsYou { suggestion, problem, .. } = rule(mode, judged(Action::Answer, keys, text))
                else {
                    panic!("{mode:?} applied an answer that says nothing");
                };
                assert_eq!(suggestion, None);
                assert_eq!(problem.as_deref(), Some(UNUSABLE));
            }
        }
    }

    #[test]
    fn the_labels_of_a_verdict_are_bounded_single_lines() {
        let long = Verdict {
            summary: "s".repeat(MAX_SUMMARY_CHARS * 2),
            reason: "r".repeat(MAX_REASON_CHARS * 2),
            tag: Some("t".repeat(MAX_TAG_CHARS * 2)),
            ..judged(Action::Escalate, Some(keys(&["1"])), None)
        };
        let Ruling::NeedsYou { suggestion: Some(suggestion), tag: Some(tag), .. } = rule(Mode::Observe, long) else {
            panic!()
        };
        assert_eq!(suggestion.summary.chars().count(), MAX_SUMMARY_CHARS);
        assert_eq!(suggestion.reason.chars().count(), MAX_REASON_CHARS);
        assert_eq!(tag.chars().count(), MAX_TAG_CHARS);
        let blank = Verdict { tag: Some("  ".into()), ..judged(Action::Escalate, None, None) };
        assert_eq!(rule(Mode::Moderate, blank), Ruling::NeedsYou { suggestion: None, tag: None, problem: None });
    }

    #[test]
    fn a_chat_answer_reads_its_reply_the_agents_to_read_and_each_action() {
        let answer: ChatAnswer = first_object(
            r#"```json
{"reply":" Done. ","read":[3,3,5,6,7,8],"actions":[
 {"type":"send","agent":3,"text":"run the tests"},
 {"type":"answer","decision":9,"keys":["1"]},
 {"type":"answer","decision":9,"text":"yes"},
 {"type":"stop","agent":4},
 {"type":"start","kind":"claude","cwd":"~/dev/app","task":"fix it","session":null},
 {"type":"launch","agent":1},
 {"type":"send","agent":"three"}
]}
```"#,
        )
        .unwrap();
        assert_eq!(answer.said(), "Done.");
        assert_eq!(answer.reads(), [3, 5, 6, 7], "each agent once, four at most");
        assert_eq!(
            answer.actions(),
            [
                Some(ChatAction::Send { agent: 3, text: "run the tests".into() }),
                Some(ChatAction::Answer { decision: 9, keys: Some(keys(&["1"])), text: None }),
                Some(ChatAction::Answer { decision: 9, keys: None, text: Some("yes".into()) }),
                Some(ChatAction::Stop { agent: 4 }),
                Some(ChatAction::Start {
                    kind: "claude".into(),
                    cwd: "~/dev/app".into(),
                    task: "fix it".into(),
                    session: None,
                    model: None
                }),
                None,
                None,
            ],
            "an action written wrongly is skipped without losing the others"
        );
    }

    #[test]
    fn a_chat_answer_can_start_with_a_model_and_restart_remove_or_rename_an_agent() {
        let answer: ChatAnswer = first_object(
            r#"{"reply":"ok","actions":[
 {"type":"start","kind":"claude","cwd":"~/dev/app","model":"fable","session":"work"},
 {"type":"restart","agent":5},
 {"type":"remove","agent":6},
 {"type":"rename","agent":7,"name":"api"},
 {"type":"rename","agent":7},
 {"type":"remove"}
]}"#,
        )
        .unwrap();
        assert_eq!(
            answer.actions(),
            [
                Some(ChatAction::Start {
                    kind: "claude".into(),
                    cwd: "~/dev/app".into(),
                    task: String::new(),
                    session: Some("work".into()),
                    model: Some("fable".into())
                }),
                Some(ChatAction::Restart { agent: 5 }),
                Some(ChatAction::Remove { agent: 6 }),
                Some(ChatAction::Rename { agent: 7, name: "api".into() }),
                None,
                None,
            ]
        );
    }

    #[test]
    fn a_chat_answer_may_only_talk_and_its_actions_are_bounded() {
        let talk: ChatAnswer = first_object(r#"{"reply":"All quiet.","read":null,"actions":null}"#).unwrap();
        assert_eq!((talk.said().as_str(), talk.reads().len(), talk.actions().len()), ("All quiet.", 0, 0));
        let bare: ChatAnswer = first_object(r#"{"reply":"hi"}"#).unwrap();
        assert!(bare.reads().is_empty() && bare.actions().is_empty());
        let stops: Vec<String> =
            (0..MAX_ACTIONS + 4).map(|agent| format!(r#"{{"type":"stop","agent":{agent}}}"#)).collect();
        let many: ChatAnswer = first_object(&format!(r#"{{"reply":"","actions":[{}]}}"#, stops.join(","))).unwrap();
        assert_eq!(many.actions().len(), MAX_ACTIONS);
        let long: ChatAnswer = first_object(&format!(r#"{{"reply":"{}"}}"#, "x".repeat(MAX_REPLY_CHARS + 5))).unwrap();
        assert_eq!(long.said().chars().count(), MAX_REPLY_CHARS);
        assert_eq!(first_object::<ChatAnswer>(r#"{"reply":3}"#), None);
    }
}
