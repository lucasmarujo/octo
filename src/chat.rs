use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::launch::is_safe_session_id;
use crate::subagents::claude_project_slug;

pub const WINDOW_BYTES: u64 = 1024 * 1024;
pub const FIRST_WINDOW_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_ITEMS: usize = 240;
pub const PAGE_MAX_BYTES: usize = 256 * 1024;
pub const TEXT_MAX_CHARS: usize = 6_000;
pub const DETAIL_MAX_CHARS: usize = 160;
pub const BODY_MAX_LINES: usize = 48;
pub const LINE_MAX_CHARS: usize = 200;
const PROJECTS_DIR: &str = "projects";
const TRANSCRIPT_SUFFIX: &str = ".jsonl";
const USER: &str = "user";
const USER_TYPE: &str = "\"type\":\"user\"";
const ASSISTANT_TYPE: &str = "\"type\":\"assistant\"";
const ASSISTANT: &str = "assistant";
const WRAPPED: char = '<';
const DETAIL_FIELDS: &[&str] =
    &["file_path", "notebook_path", "command", "pattern", "description", "query", "url", "prompt", "skill", "path"];
const COMMAND_FIELD: &str = "command";
const OLD_FIELD: &str = "old_string";
const NEW_FIELD: &str = "new_string";
const CONTENT_FIELD: &str = "content";
const EDITS_FIELD: &str = "edits";
const TODOS_FIELD: &str = "todos";
const TODO_DONE: &str = "completed";
const TODO_ACTIVE: &str = "in_progress";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatItem {
    User { text: String },
    Text { text: String },
    Step { id: String, tool: String, detail: String, body: Vec<String> },
    Result { id: String, failed: bool, summary: String, body: Vec<String> },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatPage {
    pub items: Vec<ChatItem>,
    pub next: u64,
    pub reset: bool,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Record {
    #[serde(rename = "type")]
    kind: String,
    is_meta: bool,
    is_sidechain: bool,
    is_compact_summary: bool,
    message: Option<Message>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Message {
    content: Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    id: Option<String>,
    name: Option<String>,
    input: Value,
    text: Option<String>,
    tool_use_id: Option<String>,
    content: Value,
    is_error: Option<bool>,
}

pub fn claude_transcript(claude_home: &Path, cwd: &Path, session_id: &str) -> Option<PathBuf> {
    if !is_safe_session_id(session_id) {
        return None;
    }
    let projects = claude_home.join(PROJECTS_DIR);
    let name = format!("{session_id}{TRANSCRIPT_SUFFIX}");
    let direct = projects.join(claude_project_slug(cwd)).join(&name);
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(&projects).ok()?.flatten().map(|entry| entry.path().join(&name)).find(|path| path.is_file())
}

pub fn read_claude(path: &Path, from: u64) -> ChatPage {
    let Ok(len) = fs::metadata(path).map(|meta| meta.len()) else { return ChatPage::default() };
    let rewound = from > len;
    let wanted = if rewound { 0 } else { from };
    let first = wanted == 0;
    let limit = if first { FIRST_WINDOW_BYTES } else { WINDOW_BYTES };
    let start = if first { len.saturating_sub(limit) } else { wanted };
    let Some(bytes) = window(path, start, limit) else { return ChatPage { next: wanted, ..ChatPage::default() } };
    let skipped = if first && start > 0 {
        bytes.iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |at| at + 1)
    } else {
        0
    };
    let ended = bytes.iter().rposition(|byte| *byte == b'\n').map(|at| at + 1).filter(|end| *end >= skipped);
    let complete = match ended {
        Some(end) => end,
        None if bytes.len() as u64 == limit => bytes.len(),
        None => skipped,
    };
    let text = String::from_utf8_lossy(&bytes[skipped.min(complete)..complete]);
    let mut items: Vec<ChatItem> = text.lines().filter(|line| spoken(line)).flat_map(items_of).collect();
    let overflow = items.len().saturating_sub(MAX_ITEMS);
    items.drain(..overflow);
    let mut budget = PAGE_MAX_BYTES;
    let kept = items
        .iter()
        .rev()
        .take_while(|item| budget.checked_sub(weight(item)).map(|left| budget = left).is_some())
        .count();
    items.drain(..items.len() - kept);
    ChatPage { items, next: start + complete as u64, reset: rewound || first }
}

fn weight(item: &ChatItem) -> usize {
    let lines = |body: &[String]| body.iter().map(String::len).sum::<usize>();
    match item {
        ChatItem::User { text } | ChatItem::Text { text } => text.len(),
        ChatItem::Step { id, tool, detail, body } => id.len() + tool.len() + detail.len() + lines(body),
        ChatItem::Result { id, summary, body, .. } => id.len() + summary.len() + lines(body),
    }
}

fn spoken(line: &str) -> bool {
    line.contains(USER_TYPE) || line.contains(ASSISTANT_TYPE)
}

fn window(path: &Path, start: u64, limit: u64) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn items_of(line: &str) -> Vec<ChatItem> {
    let Ok(record) = serde_json::from_str::<Record>(line) else { return Vec::new() };
    if record.is_meta || record.is_sidechain || record.is_compact_summary {
        return Vec::new();
    }
    let Some(message) = record.message else { return Vec::new() };
    match (record.kind.as_str(), message.content) {
        (USER, Value::String(text)) => said(&text).into_iter().collect(),
        (USER, Value::Array(blocks)) => blocks.into_iter().filter_map(user_item).collect(),
        (ASSISTANT, Value::Array(blocks)) => blocks.into_iter().filter_map(assistant_item).collect(),
        _ => Vec::new(),
    }
}

fn said(text: &str) -> Option<ChatItem> {
    let text = text.trim();
    (!text.is_empty() && !text.starts_with(WRAPPED)).then(|| ChatItem::User { text: clip(text, TEXT_MAX_CHARS) })
}

fn user_item(value: Value) -> Option<ChatItem> {
    let block: Block = serde_json::from_value(value).ok()?;
    match block.kind.as_str() {
        "text" => said(&block.text?),
        "tool_result" => {
            let lines = result_lines(&block.content);
            Some(ChatItem::Result {
                id: block.tool_use_id?,
                failed: block.is_error.unwrap_or(false),
                summary: lines
                    .iter()
                    .find(|line| !line.trim().is_empty())
                    .map(|line| clip(line.trim(), DETAIL_MAX_CHARS))
                    .unwrap_or_default(),
                body: body(lines.iter().map(String::as_str)),
            })
        }
        _ => None,
    }
}

fn assistant_item(value: Value) -> Option<ChatItem> {
    let block: Block = serde_json::from_value(value).ok()?;
    match block.kind.as_str() {
        "text" => {
            let text = block.text?;
            let text = text.trim();
            (!text.is_empty()).then(|| ChatItem::Text { text: clip(text, TEXT_MAX_CHARS) })
        }
        "tool_use" => Some(ChatItem::Step {
            id: block.id?,
            tool: block.name?,
            detail: detail(&block.input),
            body: step_body(&block.input),
        }),
        _ => None,
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

fn body<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    lines.take(BODY_MAX_LINES).map(|line| clip(line.trim_end(), LINE_MAX_CHARS)).collect()
}

fn detail(input: &Value) -> String {
    DETAIL_FIELDS
        .iter()
        .find_map(|field| input.get(*field).and_then(Value::as_str))
        .map(|text| clip(text.lines().next().unwrap_or_default().trim(), DETAIL_MAX_CHARS))
        .unwrap_or_default()
}

fn marked(mark: char, text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().map(move |line| format!("{mark} {line}"))
}

fn change(old: Option<&str>, new: Option<&str>) -> Vec<String> {
    marked('-', old.unwrap_or_default()).chain(marked('+', new.unwrap_or_default())).collect()
}

fn step_body(input: &Value) -> Vec<String> {
    let text = |field: &str| input.get(field).and_then(Value::as_str);
    let lines: Vec<String> = if let Some(edits) = input.get(EDITS_FIELD).and_then(Value::as_array) {
        edits
            .iter()
            .flat_map(|edit| {
                change(edit.get(OLD_FIELD).and_then(Value::as_str), edit.get(NEW_FIELD).and_then(Value::as_str))
            })
            .collect()
    } else if text(OLD_FIELD).is_some() || text(NEW_FIELD).is_some() {
        change(text(OLD_FIELD), text(NEW_FIELD))
    } else if let Some(todos) = input.get(TODOS_FIELD).and_then(Value::as_array) {
        todos.iter().filter_map(todo_line).collect()
    } else if let Some(command) = text(COMMAND_FIELD) {
        command.lines().map(str::to_string).collect()
    } else if let Some(content) = text(CONTENT_FIELD) {
        marked('+', content).collect()
    } else {
        Vec::new()
    };
    body(lines.iter().map(String::as_str))
}

fn todo_line(todo: &Value) -> Option<String> {
    let content = todo.get(CONTENT_FIELD).and_then(Value::as_str)?;
    let mark = match todo.get("status").and_then(Value::as_str) {
        Some(TODO_DONE) => '✓',
        Some(TODO_ACTIVE) => '›',
        _ => '·',
    };
    Some(format!("{mark} {content}"))
}

fn result_lines(content: &Value) -> Vec<String> {
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            parts.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    };
    text.lines().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const SESSION: &str = "0f6f1d4e-8a55-4b0c-9d53-2f0c6f7a9b11";

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orkai-chat-{tag}-{}-{}", std::process::id(), crate::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn line(value: serde_json::Value) -> String {
        format!("{value}\n")
    }

    fn user(text: &str) -> String {
        line(serde_json::json!({ "type": "user", "message": { "role": "user", "content": text } }))
    }

    fn says(text: &str) -> String {
        line(serde_json::json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": text }] } }))
    }

    fn uses(id: &str, name: &str, input: serde_json::Value) -> String {
        line(
            serde_json::json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } }),
        )
    }

    fn returns(id: &str, content: serde_json::Value, failed: bool) -> String {
        line(
            serde_json::json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": failed }] } }),
        )
    }

    fn transcript(dir: &Path, text: &str) -> PathBuf {
        let path = dir.join(format!("{SESSION}.jsonl"));
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_conversation_reads_as_what_was_asked_what_was_said_and_each_step_with_its_result() {
        let dir = temp_dir("case1");
        let text = [
            user("Fix the flaky holder e2e test on mac"),
            uses("t1", "Read", serde_json::json!({ "file_path": "tests/holder_e2e.rs" })),
            returns("t1", serde_json::json!([{ "type": "text", "text": "\n  1 use std::fs;\n  2 fn main() {}" }]), false),
            says("The test reads the runtime file before the holder writes it."),
            uses("t2", "Edit", serde_json::json!({ "file_path": "tests/holder_e2e.rs", "old_string": "sleep(500);\nread()", "new_string": "wait_for(&path)" })),
            returns("t2", serde_json::json!("The file has been updated"), false),
            uses("t3", "Bash", serde_json::json!({ "command": "cargo test --test holder_e2e", "description": "Run the test" })),
            returns("t3", serde_json::json!("error: test failed"), true),
        ]
        .concat();
        let page = read_claude(&transcript(&dir, &text), 0);
        assert_eq!(
            page.items,
            vec![
                ChatItem::User { text: "Fix the flaky holder e2e test on mac".into() },
                ChatItem::Step {
                    id: "t1".into(),
                    tool: "Read".into(),
                    detail: "tests/holder_e2e.rs".into(),
                    body: vec![]
                },
                ChatItem::Result {
                    id: "t1".into(),
                    failed: false,
                    summary: "1 use std::fs;".into(),
                    body: vec!["".into(), "  1 use std::fs;".into(), "  2 fn main() {}".into()]
                },
                ChatItem::Text { text: "The test reads the runtime file before the holder writes it.".into() },
                ChatItem::Step {
                    id: "t2".into(),
                    tool: "Edit".into(),
                    detail: "tests/holder_e2e.rs".into(),
                    body: vec!["- sleep(500);".into(), "- read()".into(), "+ wait_for(&path)".into()],
                },
                ChatItem::Result {
                    id: "t2".into(),
                    failed: false,
                    summary: "The file has been updated".into(),
                    body: vec!["The file has been updated".into()]
                },
                ChatItem::Step {
                    id: "t3".into(),
                    tool: "Bash".into(),
                    detail: "cargo test --test holder_e2e".into(),
                    body: vec!["cargo test --test holder_e2e".into()]
                },
                ChatItem::Result {
                    id: "t3".into(),
                    failed: true,
                    summary: "error: test failed".into(),
                    body: vec!["error: test failed".into()]
                },
            ]
        );
        assert_eq!(page.next, text.len() as u64);
        assert!(page.reset, "the first page replaces whatever the phone had");
    }

    #[test]
    fn what_the_person_never_typed_and_what_a_subagent_said_stay_out() {
        let dir = temp_dir("case2");
        let text = [
            line(serde_json::json!({ "type": "user", "isMeta": true, "message": { "content": "Caveat: the messages below were generated" } })),
            user("<command-name>/clear</command-name>"),
            user("<system-reminder>context</system-reminder>"),
            line(serde_json::json!({ "type": "user", "isCompactSummary": true, "message": { "content": "This session is being continued" } })),
            line(serde_json::json!({ "type": "assistant", "isSidechain": true, "message": { "content": [{ "type": "text", "text": "from a subagent" }] } })),
            line(serde_json::json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "private" }] } })),
            line(serde_json::json!({ "type": "ai-title", "aiTitle": "a title" })),
            "not json at all\n".to_string(),
            says("   "),
            user("really typed"),
        ]
        .concat();
        let page = read_claude(&transcript(&dir, &text), 0);
        assert_eq!(page.items, vec![ChatItem::User { text: "really typed".into() }]);
    }

    #[test]
    fn the_next_page_starts_where_the_last_complete_line_ended() {
        let dir = temp_dir("case3");
        let first = user("one");
        let path = transcript(&dir, &format!("{first}{}", &says("half a li")[..20]));
        let page = read_claude(&path, 0);
        assert_eq!(page.items, vec![ChatItem::User { text: "one".into() }]);
        assert_eq!(page.next, first.len() as u64, "a line still being written waits for the next read");

        fs::write(&path, format!("{first}{}{}", says("two"), user("three"))).unwrap();
        let more = read_claude(&path, page.next);
        assert_eq!(more.items, vec![ChatItem::Text { text: "two".into() }, ChatItem::User { text: "three".into() }]);
        assert!(!more.reset);

        let same = read_claude(&path, more.next);
        assert_eq!((same.items.len(), same.next, same.reset), (0, more.next, false));

        fs::write(&path, user("a new file")).unwrap();
        let again = read_claude(&path, more.next);
        assert!(again.reset, "a transcript that shrank is read again from its start");
        assert_eq!(again.items, vec![ChatItem::User { text: "a new file".into() }]);
    }

    #[test]
    fn a_long_conversation_sends_only_its_end_and_never_a_line_cut_in_half() {
        let dir = temp_dir("case4");
        let path = &dir.join(format!("{SESSION}.jsonl"));
        let mut file = File::create(path).unwrap();
        let filler = "x".repeat(900);
        let lines = 2_000;
        for index in 0..lines {
            file.write_all(says(&format!("message {index} {filler}")).as_bytes()).unwrap();
        }
        drop(file);
        let page = read_claude(path, 0);
        assert!(page.items.len() <= MAX_ITEMS);
        assert!(
            page.items.iter().map(weight).sum::<usize>() <= PAGE_MAX_BYTES,
            "a page never outgrows what a phone should receive at once"
        );
        assert_eq!(page.next, fs::metadata(path).unwrap().len());
        let ChatItem::Text { text } = page.items.last().unwrap() else { panic!("a text ends it") };
        assert!(text.starts_with(&format!("message {} ", lines - 1)));
        assert!(page.items.iter().all(|item| matches!(item, ChatItem::Text { text } if text.starts_with("message "))));
    }

    #[test]
    fn long_texts_long_outputs_and_long_lines_are_cut_before_they_travel() {
        let dir = temp_dir("case5");
        let output = (0..500).map(|index| format!("line {index} {}", "y".repeat(400))).collect::<Vec<_>>().join("\n");
        let text = [
            says(&"z".repeat(TEXT_MAX_CHARS * 2)),
            uses("t1", "Bash", serde_json::json!({ "command": "ls" })),
            returns("t1", serde_json::json!(output), false),
        ]
        .concat();
        let page = read_claude(&transcript(&dir, &text), 0);
        let ChatItem::Text { text } = &page.items[0] else { panic!("a text") };
        assert_eq!(text.chars().count(), TEXT_MAX_CHARS);
        assert!(text.ends_with('…'));
        let ChatItem::Result { body, summary, .. } = &page.items[2] else { panic!("a result") };
        assert_eq!(body.len(), BODY_MAX_LINES);
        assert!(body.iter().all(|line| line.chars().count() <= LINE_MAX_CHARS));
        assert!(summary.chars().count() <= DETAIL_MAX_CHARS);
    }

    #[test]
    fn a_plan_reads_as_a_checklist_and_a_new_file_as_added_lines() {
        let dir = temp_dir("case6");
        let todos = serde_json::json!({ "todos": [
            { "content": "Read the test", "status": "completed" },
            { "content": "Fix it", "status": "in_progress" },
            { "content": "Run it", "status": "pending" },
        ] });
        let text = [
            uses("t1", "TodoWrite", todos),
            uses("t2", "Write", serde_json::json!({ "file_path": "notes.md", "content": "one\ntwo" })),
        ]
        .concat();
        let page = read_claude(&transcript(&dir, &text), 0);
        assert_eq!(
            page.items,
            vec![
                ChatItem::Step {
                    id: "t1".into(),
                    tool: "TodoWrite".into(),
                    detail: String::new(),
                    body: vec!["✓ Read the test".into(), "› Fix it".into(), "· Run it".into()]
                },
                ChatItem::Step {
                    id: "t2".into(),
                    tool: "Write".into(),
                    detail: "notes.md".into(),
                    body: vec!["+ one".into(), "+ two".into()]
                },
            ]
        );
    }

    #[test]
    fn the_transcript_is_found_in_the_folder_of_the_project_or_in_any_other() {
        let home = temp_dir("case7");
        let cwd = Path::new("/work/octo");
        let own = &home.join("projects").join(claude_project_slug(cwd));
        fs::create_dir_all(own).unwrap();
        assert_eq!(claude_transcript(&home, cwd, SESSION), None);
        let elsewhere = &home.join("projects").join("-another-folder");
        fs::create_dir_all(elsewhere).unwrap();
        fs::write(elsewhere.join(format!("{SESSION}.jsonl")), "").unwrap();
        assert_eq!(claude_transcript(&home, cwd, SESSION), Some(elsewhere.join(format!("{SESSION}.jsonl"))));
        fs::write(own.join(format!("{SESSION}.jsonl")), "").unwrap();
        assert_eq!(claude_transcript(&home, cwd, SESSION), Some(own.join(format!("{SESSION}.jsonl"))));
        assert_eq!(claude_transcript(&home, cwd, "../../etc/passwd"), None, "an id is never a path");
    }

    #[test]
    fn a_line_bigger_than_a_whole_read_is_stepped_over_instead_of_stopping_the_conversation() {
        let dir = temp_dir("giant");
        let first = user("before");
        let giant =
            line(serde_json::json!({ "type": "api-request-blob", "blob": "x".repeat(WINDOW_BYTES as usize * 2) }));
        let path = transcript(&dir, &format!("{first}{giant}{}", says("after")));
        let mut from = first.len() as u64;
        let mut seen = Vec::new();
        for _ in 0..6 {
            let page = read_claude(&path, from);
            assert!(page.next >= from);
            from = page.next;
            seen.extend(page.items);
        }
        assert_eq!(from, fs::metadata(&path).unwrap().len());
        assert_eq!(seen, vec![ChatItem::Text { text: "after".into() }]);
    }

    #[test]
    fn a_missing_transcript_is_an_empty_page() {
        assert_eq!(read_claude(Path::new("/nowhere/at/all.jsonl"), 12), ChatPage::default());
    }
}
