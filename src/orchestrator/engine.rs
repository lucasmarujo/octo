use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::launch::{self, Reply};
use crate::model::AgentKind;
use crate::orchestrator::logs::push_bounded;
use crate::orchestrator::{ENGINES, one_line};
use crate::paths::{Home, create_private_dir};
use crate::util::lock;

pub const ENGINE_TIMEOUT: Duration = Duration::from_secs(120);
pub const MAX_ENGINE_OUTPUT_BYTES: u64 = 256 * 1024;
pub const MAX_PROBLEM_CHARS: usize = 200;
pub const MAX_REMEMBERED_STOPS: usize = 16;
pub const STOPPED: &str = "the engine was stopped";
const WORK_DIR: &str = "work";
const CLAUDE_ARGS: [&str; 9] = [
    "-p",
    "--output-format",
    "json",
    "--no-session-persistence",
    "--tools",
    "",
    "--strict-mcp-config",
    "--setting-sources",
    "",
];
const CLAUDE_MODEL_OPTION: &str = "--model";
const CODEX_ARGS: [&str; 7] = ["exec", "--skip-git-repo-check", "--ephemeral", "-s", "read-only", "--color", "never"];
const CODEX_MODEL_OPTION: &str = "-m";
const CODEX_PROMPT_FROM_STDIN: &str = "-";
const MODEL_OPTION: &str = "--model";
const ANTIGRAVITY_ARGS: [&str; 7] = [
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--disable-slash-commands",
    "--sandbox",
    "--print=",
];
const PI_ARGS: [&str; 7] =
    ["-p", "--no-session", "--no-tools", "--no-context-files", "--no-skills", "--no-prompt-templates", "--offline"];
const CURSOR_ARGS: [&str; 6] = ["-p", "--output-format", "json", "--mode", "ask", "--trust"];
const RUN_ARGS: [&str; 3] = ["run", "--format", "json"];
const QWEN_ARGS: [&str; 4] = ["-o", "json", "--chat-recording", "false"];
const GROK_ARGS: [&str; 3] = ["--no-auto-update", "--output-format", "plain"];
const GROK_PROMPT_FILE_OPTION: &str = "--prompt-file";
const KIMI_ARGS: [&str; 2] = ["--output-format", "stream-json"];
const KIMI_PROMPT_OPTION: &str = "--prompt";
const KIMI_LOG_HINT: &str = "See log:";
const PROMPTS_DIR: &str = "prompts";
const PROMPT_FILE_EXTENSION: &str = "txt";
const STREAM_USER_EVENT: &str = "user";
const STREAM_RESULT_EVENT: &str = "result";
const STREAM_SUCCESS: &str = "SUCCESS";
const RUN_TEXT_EVENT: &str = "text";
const RUN_ERROR_EVENT: &str = "error";
const ASSISTANT_ROLE: &str = "assistant";

pub struct Call<'a> {
    pub ticket: u64,
    pub kind: AgentKind,
    pub model: Option<&'a str>,
    pub prompt: &'a str,
}

pub trait Engine: Send + Sync {
    fn ask(&self, config: &Config, call: &Call) -> Result<String, String>;
    fn stop(&self, ticket: u64);
    fn stop_all(&self);
}

pub fn engine_args(kind: AgentKind, model: Option<&str>) -> Option<Vec<String>> {
    let owned = |args: &[&str]| args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
    match kind {
        AgentKind::Claude => {
            let mut args = owned(&CLAUDE_ARGS);
            args.extend(model.into_iter().flat_map(|model| [CLAUDE_MODEL_OPTION.to_string(), model.to_string()]));
            Some(args)
        }
        AgentKind::Codex => {
            let mut args = owned(&CODEX_ARGS);
            args.extend(model.into_iter().flat_map(|model| [CODEX_MODEL_OPTION.to_string(), model.to_string()]));
            args.push(CODEX_PROMPT_FROM_STDIN.to_string());
            Some(args)
        }
        AgentKind::Kilo | AgentKind::Opencode => Some(with_model(model, owned(&RUN_ARGS))),
        AgentKind::Antigravity => Some(with_model(model, owned(&ANTIGRAVITY_ARGS))),
        AgentKind::Grok => Some(with_model(model, owned(&GROK_ARGS))),
        AgentKind::Qwen => Some(with_model(model, owned(&QWEN_ARGS))),
        AgentKind::Kimi => Some(with_model(model, owned(&KIMI_ARGS))),
        AgentKind::Pi => Some(with_model(model, owned(&PI_ARGS))),
        AgentKind::Cursor => Some(with_model(model, owned(&CURSOR_ARGS))),
        AgentKind::Terminal => None,
    }
}

fn with_model(model: Option<&str>, args: Vec<String>) -> Vec<String> {
    let mut args = args;
    args.extend(model.into_iter().flat_map(|model| [MODEL_OPTION.to_string(), model.to_string()]));
    args
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Stdin,
    Stream,
    Argument,
    File,
}

pub fn delivery(kind: AgentKind) -> Delivery {
    match kind {
        AgentKind::Antigravity => Delivery::Stream,
        AgentKind::Kimi => Delivery::Argument,
        AgentKind::Grok => Delivery::File,
        _ => Delivery::Stdin,
    }
}

pub fn prompt_args(kind: AgentKind, prompt: &str, file: Option<&Path>) -> Vec<String> {
    match delivery(kind) {
        Delivery::Argument => vec![format!("{KIMI_PROMPT_OPTION}={prompt}")],
        Delivery::File => file
            .map(|file| vec![GROK_PROMPT_FILE_OPTION.to_string(), file.to_string_lossy().into_owned()])
            .unwrap_or_default(),
        Delivery::Stdin | Delivery::Stream => Vec::new(),
    }
}

#[derive(Serialize)]
struct StreamMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct StreamInput<'a> {
    event: &'a str,
    message: StreamMessage<'a>,
}

pub fn engine_input(kind: AgentKind, prompt: &str) -> Vec<u8> {
    match delivery(kind) {
        Delivery::Stream => {
            let message = StreamMessage { role: STREAM_USER_EVENT, content: prompt };
            let line = serde_json::to_string(&StreamInput { event: STREAM_USER_EVENT, message }).unwrap_or_default();
            format!("{line}\n").into_bytes()
        }
        Delivery::Stdin => prompt.as_bytes().to_vec(),
        Delivery::Argument | Delivery::File => Vec::new(),
    }
}

pub fn command_line(config: &Config, kind: AgentKind, model: Option<&str>) -> Result<(String, Vec<String>), String> {
    let headless = engine_args(kind, model).filter(|_| ENGINES.contains(&kind));
    let (Some(headless), Some((command, own))) = (headless, config.agent(kind)) else {
        return Err(format!("{} cannot be Octo's engine", kind.label()));
    };
    let own = if model.is_some() { launch::without_model(own.to_vec(), kind) } else { own.to_vec() };
    Ok((command.to_string(), own.into_iter().chain(headless).collect()))
}

#[derive(Deserialize)]
struct ResultOutput {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: bool,
}

#[derive(Deserialize)]
struct StreamOutput {
    event: String,
    #[serde(default)]
    result: Option<StreamResult>,
}

#[derive(Deserialize)]
struct StreamResult {
    #[serde(default)]
    status: String,
    #[serde(default)]
    response: String,
    #[serde(default)]
    error: String,
}

fn problem(kind: AgentKind, said: &str) -> String {
    let said = one_line(said, MAX_PROBLEM_CHARS);
    if said.is_empty() { format!("{} gave no answer", kind.label()) } else { format!("{}: {said}", kind.label()) }
}

fn last_line(text: &str) -> &str {
    text.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default()
}

pub fn result_answer(kind: AgentKind, reply: &Reply) -> Result<String, String> {
    let complaint = last_line(&reply.errors);
    let Ok(parsed) = serde_json::from_str::<ResultOutput>(last_line(&reply.output)) else {
        let said = if complaint.is_empty() { reply.output.as_str() } else { complaint };
        return Err(problem(kind, said));
    };
    match parsed.result.filter(|result| !result.trim().is_empty()) {
        Some(result) if !parsed.is_error => Ok(result),
        Some(refusal) => Err(problem(kind, &refusal)),
        None => Err(problem(kind, complaint)),
    }
}

pub fn stream_answer(kind: AgentKind, reply: &Reply) -> Result<String, String> {
    let complaint = last_line(&reply.errors);
    let result = reply
        .output
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<StreamOutput>(line.trim()).ok())
        .find(|line| line.event == STREAM_RESULT_EVENT)
        .and_then(|line| line.result);
    let Some(result) = result else { return Err(problem(kind, complaint)) };
    match (result.status == STREAM_SUCCESS, result.response.trim().is_empty()) {
        (true, false) => Ok(result.response),
        _ if !result.error.trim().is_empty() => Err(problem(kind, &result.error)),
        _ => Err(problem(kind, complaint)),
    }
}

#[derive(Deserialize)]
struct RunEvent {
    #[serde(rename = "type")]
    event: String,
    #[serde(default)]
    part: Option<RunPart>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RunPart {
    #[serde(default)]
    text: String,
}

fn said_in(value: &serde_json::Value) -> String {
    ["/message", "/data/message", "/name"]
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(serde_json::Value::as_str))
        .or_else(|| value.as_str())
        .unwrap_or_default()
        .to_string()
}

pub fn run_answer(kind: AgentKind, reply: &Reply) -> Result<String, String> {
    let events: Vec<RunEvent> =
        reply.output.lines().filter_map(|line| serde_json::from_str::<RunEvent>(line.trim()).ok()).collect();
    let text = events
        .iter()
        .rev()
        .filter(|event| event.event == RUN_TEXT_EVENT)
        .filter_map(|event| event.part.as_ref())
        .find(|part| !part.text.trim().is_empty());
    if let Some(part) = text {
        return Ok(part.text.clone());
    }
    let failure =
        events.iter().rev().filter(|event| event.event == RUN_ERROR_EVENT).find_map(|event| event.error.as_ref());
    let said = failure.map(said_in).filter(|said| !said.trim().is_empty());
    Err(problem(kind, said.as_deref().unwrap_or(last_line(&reply.errors))))
}

#[derive(Deserialize)]
struct QwenItem {
    #[serde(rename = "type", default)]
    item: String,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

pub fn qwen_answer(kind: AgentKind, reply: &Reply) -> Result<String, String> {
    let complaint = last_line(&reply.errors);
    let items = serde_json::from_str::<Vec<QwenItem>>(reply.output.trim()).unwrap_or_default();
    let Some(result) = items.into_iter().rev().find(|item| item.item == STREAM_RESULT_EVENT) else {
        return Err(problem(kind, complaint));
    };
    let answer = result.result.filter(|answer| !answer.trim().is_empty());
    match (result.is_error, answer) {
        (false, Some(answer)) => Ok(answer),
        _ => {
            let said = result.error.as_ref().map(said_in).filter(|said| !said.trim().is_empty());
            Err(problem(kind, said.as_deref().unwrap_or(complaint)))
        }
    }
}

#[derive(Deserialize)]
struct KimiLine {
    #[serde(default)]
    role: String,
    #[serde(rename = "type", default)]
    line: String,
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    message: Option<String>,
}

fn content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get(RUN_TEXT_EVENT).and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

pub fn kimi_answer(kind: AgentKind, reply: &Reply) -> Result<String, String> {
    let lines: Vec<KimiLine> =
        reply.output.lines().filter_map(|line| serde_json::from_str::<KimiLine>(line.trim()).ok()).collect();
    let answer = lines
        .iter()
        .rev()
        .filter(|line| line.role == ASSISTANT_ROLE && line.tool_calls.as_ref().is_none_or(Vec::is_empty))
        .filter_map(|line| line.content.as_ref().map(content_text))
        .find(|text| !text.trim().is_empty());
    if let Some(answer) = answer {
        return Ok(answer);
    }
    let failure = lines.iter().rev().filter(|line| line.line == RUN_ERROR_EVENT).find_map(|line| line.message.clone());
    let complaint = reply
        .errors
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with(KIMI_LOG_HINT))
        .unwrap_or_default();
    Err(problem(kind, failure.as_deref().unwrap_or(complaint)))
}

pub fn answer_of(kind: AgentKind, reply: &Reply) -> Result<String, String> {
    match kind {
        AgentKind::Claude | AgentKind::Cursor => result_answer(kind, reply),
        AgentKind::Antigravity => stream_answer(kind, reply),
        AgentKind::Kilo | AgentKind::Opencode => run_answer(kind, reply),
        AgentKind::Qwen => qwen_answer(kind, reply),
        AgentKind::Kimi => kimi_answer(kind, reply),
        _ if reply.output.trim().is_empty() => Err(problem(kind, last_line(&reply.errors))),
        _ => Ok(reply.output.clone()),
    }
}

pub struct CliEngine {
    work: PathBuf,
    prompts: PathBuf,
    timeout: Duration,
    running: Mutex<Vec<(u64, u32)>>,
    stopped: Mutex<VecDeque<u64>>,
    closing: AtomicBool,
    written: AtomicU64,
}

impl CliEngine {
    pub fn new(home: &Home) -> Self {
        Self::with_timeout(home, ENGINE_TIMEOUT)
    }

    pub fn with_timeout(home: &Home, timeout: Duration) -> Self {
        Self {
            work: home.orchestrator_dir().join(WORK_DIR),
            prompts: home.orchestrator_dir().join(PROMPTS_DIR),
            timeout,
            running: Mutex::new(Vec::new()),
            stopped: Mutex::new(VecDeque::new()),
            closing: AtomicBool::new(false),
            written: AtomicU64::new(0),
        }
    }

    fn prompt_file(&self, call: &Call) -> Result<Option<PathBuf>, String> {
        if delivery(call.kind) != Delivery::File {
            return Ok(None);
        }
        create_private_dir(&self.prompts).map_err(|err| err.to_string())?;
        let count = self.written.fetch_add(1, Ordering::SeqCst);
        let file = self.prompts.join(format!("{}-{count}.{PROMPT_FILE_EXTENSION}", std::process::id()));
        fs::write(&file, call.prompt).map_err(|err| err.to_string())?;
        Ok(Some(file))
    }

    fn run(&self, config: &Config, call: &Call, file: Option<&Path>) -> Result<String, String> {
        let (command, mut args) = command_line(config, call.kind, call.model)?;
        args.extend(prompt_args(call.kind, call.prompt, file));
        let program = launch::resolve_program(&command).map_err(|err| err.to_string())?;
        let (program, args) = launch::wrap_for_platform(program, args).map_err(|err| err.to_string())?;
        create_private_dir(&self.work).map_err(|err| err.to_string())?;
        let mut command = Command::new(program);
        command.args(args).current_dir(&self.work).env_remove(launch::AGENT_ID_ENV);
        let mut child = None;
        let input = engine_input(call.kind, call.prompt);
        let reply = launch::command_reply(&mut command, input, MAX_ENGINE_OUTPUT_BYTES, self.timeout, |pid| {
            lock(&self.running).push((call.ticket, pid));
            child = Some(pid);
            if self.unwanted(call.ticket) {
                launch::kill_process_group(pid);
            }
        });
        lock(&self.running).retain(|(_, pid)| Some(*pid) != child);
        let reply = reply.ok_or_else(|| format!("{} did not answer", call.kind.label()))?;
        answer_of(call.kind, &reply)
    }

    pub fn running(&self) -> usize {
        lock(&self.running).len()
    }

    fn kill(&self, wanted: impl Fn(u64) -> bool) -> usize {
        let pids: Vec<u32> =
            lock(&self.running).iter().filter(|(ticket, _)| wanted(*ticket)).map(|(_, pid)| *pid).collect();
        for pid in &pids {
            launch::kill_process_group(*pid);
        }
        pids.len()
    }

    fn unwanted(&self, ticket: u64) -> bool {
        if self.closing.load(Ordering::SeqCst) {
            return true;
        }
        let mut stopped = lock(&self.stopped);
        let found = stopped.iter().position(|stopped| *stopped == ticket);
        found.is_some_and(|at| stopped.remove(at).is_some())
    }
}

impl Engine for CliEngine {
    fn ask(&self, config: &Config, call: &Call) -> Result<String, String> {
        if self.unwanted(call.ticket) {
            return Err(STOPPED.to_string());
        }
        let file = self.prompt_file(call)?;
        let answer = self.run(config, call, file.as_deref());
        if let Some(file) = file {
            let _ = fs::remove_file(file);
        }
        answer
    }

    fn stop(&self, ticket: u64) {
        if self.kill(|running| running == ticket) == 0 {
            push_bounded(&mut lock(&self.stopped), ticket, MAX_REMEMBERED_STOPS);
        }
    }

    fn stop_all(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.kill(|_| true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;

    #[test]
    fn claude_runs_headless_with_no_tools_no_settings_and_the_prompt_on_stdin() {
        assert_eq!(
            engine_args(AgentKind::Claude, None).unwrap(),
            [
                "-p",
                "--output-format",
                "json",
                "--no-session-persistence",
                "--tools",
                "",
                "--strict-mcp-config",
                "--setting-sources",
                ""
            ]
        );
        let with_model = engine_args(AgentKind::Claude, Some("opus")).unwrap();
        assert_eq!(with_model[with_model.len() - 2..], ["--model", "opus"]);
    }

    #[test]
    fn codex_runs_headless_read_only_and_reads_the_prompt_from_stdin() {
        assert_eq!(
            engine_args(AgentKind::Codex, None).unwrap(),
            ["exec", "--skip-git-repo-check", "--ephemeral", "-s", "read-only", "--color", "never", "-"]
        );
        assert_eq!(
            engine_args(AgentKind::Codex, Some("gpt-6")).unwrap(),
            ["exec", "--skip-git-repo-check", "--ephemeral", "-s", "read-only", "--color", "never", "-m", "gpt-6", "-"]
        );
        assert_eq!(engine_args(AgentKind::Terminal, None), None);
    }

    #[test]
    fn every_other_engine_runs_one_headless_turn_with_its_own_flags_and_the_chosen_model_last() {
        assert_eq!(
            engine_args(AgentKind::Antigravity, None).unwrap(),
            [
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--disable-slash-commands",
                "--sandbox",
                "--print="
            ]
        );
        assert_eq!(
            engine_args(AgentKind::Pi, None).unwrap(),
            [
                "-p",
                "--no-session",
                "--no-tools",
                "--no-context-files",
                "--no-skills",
                "--no-prompt-templates",
                "--offline"
            ]
        );
        assert_eq!(
            engine_args(AgentKind::Cursor, None).unwrap(),
            ["-p", "--output-format", "json", "--mode", "ask", "--trust"]
        );
        assert_eq!(engine_args(AgentKind::Kilo, None).unwrap(), ["run", "--format", "json"]);
        assert_eq!(engine_args(AgentKind::Opencode, None).unwrap(), ["run", "--format", "json"]);
        assert_eq!(engine_args(AgentKind::Qwen, None).unwrap(), ["-o", "json", "--chat-recording", "false"]);
        assert_eq!(engine_args(AgentKind::Grok, None).unwrap(), ["--no-auto-update", "--output-format", "plain"]);
        assert_eq!(engine_args(AgentKind::Kimi, None).unwrap(), ["--output-format", "stream-json"]);
        for kind in ENGINES.into_iter().filter(|kind| *kind != AgentKind::Codex) {
            let chosen = engine_args(kind, Some("gemini-3.8-flash-high")).unwrap();
            assert_eq!(chosen[chosen.len() - 2..], ["--model", "gemini-3.8-flash-high"], "{kind:?}");
            assert_eq!(chosen.len(), engine_args(kind, None).unwrap().len() + 2);
        }
    }

    #[test]
    fn every_engine_has_a_headless_command_and_a_terminal_has_none() {
        for kind in ENGINES {
            assert!(engine_args(kind, None).is_some(), "{kind:?}");
        }
        assert_eq!(engine_args(AgentKind::Terminal, None), None);
    }

    #[test]
    fn each_engine_gets_the_prompt_the_way_its_program_reads_it() {
        let prompt = "say \"hi\"\nthen stop";
        let line = String::from_utf8(engine_input(AgentKind::Antigravity, prompt)).unwrap();
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1, "one line: {line}");
        let sent: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(sent, serde_json::json!({ "event": "user", "message": { "role": "user", "content": prompt } }));
        let file = Path::new("prompts").join("1-0.txt");
        for kind in [
            AgentKind::Claude,
            AgentKind::Codex,
            AgentKind::Kilo,
            AgentKind::Opencode,
            AgentKind::Qwen,
            AgentKind::Pi,
            AgentKind::Cursor,
        ] {
            assert_eq!(delivery(kind), Delivery::Stdin, "{kind:?}");
            assert_eq!(engine_input(kind, prompt), prompt.as_bytes(), "{kind:?}");
            assert!(prompt_args(kind, prompt, Some(&file)).is_empty(), "{kind:?}");
        }
        assert!(prompt_args(AgentKind::Antigravity, prompt, Some(&file)).is_empty());
        assert!(engine_input(AgentKind::Kimi, prompt).is_empty(), "kimi reads no prompt on stdin");
        assert_eq!(prompt_args(AgentKind::Kimi, prompt, None), [format!("--prompt={prompt}")]);
        assert!(engine_input(AgentKind::Grok, prompt).is_empty(), "grok reads its prompt from a file");
        assert_eq!(
            prompt_args(AgentKind::Grok, prompt, Some(&file)),
            ["--prompt-file".to_string(), file.to_string_lossy().into_owned()]
        );
        assert!(prompt_args(AgentKind::Grok, prompt, None).is_empty());
    }

    #[test]
    fn the_answer_of_kilo_and_opencode_is_their_last_text_event() {
        let start = r#"{"type":"step_start","timestamp":1,"sessionID":"s"}"#;
        let early = r#"{"type":"text","timestamp":2,"sessionID":"s","part":{"text":"Let me think."}}"#;
        let last = r#"{"type":"text","timestamp":3,"sessionID":"s","part":{"text":"{\"action\":\"answer\"}"}}"#;
        let finish = r#"{"type":"step_finish","timestamp":4,"sessionID":"s"}"#;
        for kind in [AgentKind::Kilo, AgentKind::Opencode] {
            let output = format!("{start}\n{early}\n{last}\n{finish}\n");
            assert_eq!(answer_of(kind, &said(&output, "")), Ok(r#"{"action":"answer"}"#.to_string()), "{kind:?}");
        }
        let failed = r#"{"type":"error","timestamp":2,"sessionID":"s","error":{"name":"ProviderAuthError","data":{"message":"No API key for anthropic"}}}"#;
        assert_eq!(
            answer_of(AgentKind::Opencode, &said(&format!("{start}\n{failed}\n"), "")),
            Err("opencode: No API key for anthropic".to_string())
        );
        let named = r#"{"type":"error","error":{"name":"UnknownError"}}"#;
        assert_eq!(answer_of(AgentKind::Kilo, &said(named, "")), Err("kilo: UnknownError".to_string()));
        assert_eq!(
            answer_of(AgentKind::Kilo, &said("", "Error: run ended without an assistant message\n")),
            Err("kilo: Error: run ended without an assistant message".to_string())
        );
    }

    #[test]
    fn the_answer_of_qwen_is_the_result_at_the_end_of_its_json_array() {
        let output = r#"[{"type":"system","subtype":"init"},{"type":"assistant","message":{"content":[]}},{"type":"result","subtype":"success","is_error":false,"result":"{\"action\":\"escalate\"}","session_id":"s"}]"#;
        assert_eq!(answer_of(AgentKind::Qwen, &said(output, "")), Ok(r#"{"action":"escalate"}"#.to_string()));
        let failed =
            r#"[{"type":"result","is_error":true,"error":{"type":"auth","message":"Please set an Auth method"}}]"#;
        assert_eq!(answer_of(AgentKind::Qwen, &said(failed, "")), Err("qwen: Please set an Auth method".to_string()));
        assert_eq!(
            answer_of(AgentKind::Qwen, &said("", "Error when talking to the API\n")),
            Err("qwen: Error when talking to the API".to_string())
        );
    }

    #[test]
    fn the_answer_of_kimi_is_its_last_assistant_message_without_a_tool_call() {
        let tool = r#"{"role":"assistant","content":"Checking.","tool_calls":[{"id":"t1"}]}"#;
        let result = r#"{"role":"tool","content":"ok"}"#;
        let answer = r#"{"role":"assistant","content":[{"type":"text","text":"{\"action\":"},{"type":"text","text":"\"answer\"}"}]}"#;
        let meta = r#"{"role":"meta","usage":{}}"#;
        assert_eq!(
            answer_of(AgentKind::Kimi, &said(&format!("{tool}\n{result}\n{answer}\n{meta}\n"), "")),
            Ok(r#"{"action":"answer"}"#.to_string())
        );
        let plain = r#"{"role":"assistant","content":"{\"action\":\"escalate\"}"}"#;
        assert_eq!(answer_of(AgentKind::Kimi, &said(plain, "")), Ok(r#"{"action":"escalate"}"#.to_string()));
        let busy = r#"{"type":"error","code":4,"message":"The session is busy"}"#;
        assert_eq!(answer_of(AgentKind::Kimi, &said(busy, "")), Err("kimi: The session is busy".to_string()));
        assert_eq!(
            answer_of(AgentKind::Kimi, &said(tool, "No model configured. Run `kimi` and use /login\nSee log: x\n")),
            Err("kimi: No model configured. Run `kimi` and use /login".to_string()),
            "the pointer to its log explains nothing"
        );
    }

    #[test]
    fn the_answer_of_grok_is_what_it_printed() {
        assert_eq!(
            answer_of(AgentKind::Grok, &said("{\"action\":\"answer\"}\n", "")),
            Ok("{\"action\":\"answer\"}\n".to_string())
        );
        assert_eq!(answer_of(AgentKind::Grok, &said("", "Not logged in\n")), Err("grok: Not logged in".to_string()));
    }

    #[test]
    fn the_answer_of_antigravity_is_the_response_of_its_result_event() {
        let init = r#"{"event":"init","conversation_id":"c1","init":{"cwd":"work"}}"#;
        let step = r#"{"event":"step_update","step_update":{"text_delta":"{\"action\""}}"#;
        let done = r#"{"event":"result","result":{"status":"SUCCESS","response":"{\"action\":\"escalate\"}\n"}}"#;
        let output = format!("{init}\n{step}\n{done}\n");
        assert_eq!(
            answer_of(AgentKind::Antigravity, &said(&output, "")),
            Ok("{\"action\":\"escalate\"}\n".to_string())
        );
        let failed = r#"{"event":"result","result":{"status":"ERROR","response":"","error":"You are not signed in"}}"#;
        assert_eq!(
            answer_of(AgentKind::Antigravity, &said(&format!("{init}\n{failed}\n"), "error: auth")),
            Err("agy: You are not signed in".to_string())
        );
        let empty = r#"{"event":"result","result":{"status":"SUCCESS","response":" \n"}}"#;
        assert_eq!(answer_of(AgentKind::Antigravity, &said(empty, "")), Err("agy gave no answer".to_string()));
        assert_eq!(
            answer_of(AgentKind::Antigravity, &said("Usage of agy.exe:", "Error: empty prompt")),
            Err("agy: Error: empty prompt".to_string()),
            "with no result event what it complained explains it"
        );
    }

    #[test]
    fn the_answer_of_cursor_is_its_json_result_and_pi_prints_only_the_answer() {
        let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"{\"action\":\"answer\"}","session_id":"s"}"#;
        assert_eq!(
            answer_of(AgentKind::Cursor, &said(&format!("warning: slow network\n{result}\n"), "")),
            Ok(r#"{"action":"answer"}"#.to_string()),
            "a line printed before the result is skipped"
        );
        assert_eq!(
            answer_of(AgentKind::Cursor, &said("", "Error: Authentication required. Please run 'cursor-agent login'")),
            Err("cursor: Error: Authentication required. Please run 'cursor-agent login'".to_string())
        );
        assert_eq!(answer_of(AgentKind::Pi, &said("pong\n", "")), Ok("pong\n".to_string()));
        assert_eq!(
            answer_of(AgentKind::Pi, &said("", "No models available.\n")),
            Err("pi: No models available.".to_string())
        );
    }

    #[test]
    fn the_engine_runs_the_command_the_user_configured_for_that_agent() {
        let config = Config {
            claude_command: "/opt/claude".into(),
            claude_args: vec!["--model".into(), "haiku".into(), "--verbose".into()],
            codex_command: "my-codex".into(),
            ..Config::default()
        };
        let (command, args) = command_line(&config, AgentKind::Claude, None).unwrap();
        assert_eq!(command, "/opt/claude");
        assert_eq!(args[..4], ["--model", "haiku", "--verbose", "-p"], "its own arguments come first");
        let (_, chosen) = command_line(&config, AgentKind::Claude, Some("opus")).unwrap();
        assert_eq!(
            chosen.iter().filter(|arg| *arg == "--model").count(),
            1,
            "the chosen model replaces the configured one"
        );
        assert_eq!(chosen[0], "--verbose");
        assert_eq!(chosen[chosen.len() - 2..], ["--model", "opus"]);
        assert_eq!(command_line(&config, AgentKind::Codex, None).unwrap().0, "my-codex");
        let (command, args) = command_line(&config, AgentKind::Kimi, Some("kimi-k3")).unwrap();
        assert_eq!(
            (command.as_str(), args),
            ("kimi", ["--output-format", "stream-json", "--model", "kimi-k3"].map(String::from).to_vec())
        );
        let refused = command_line(&config, AgentKind::Terminal, None).unwrap_err();
        assert!(refused.contains("terminal"), "{refused}");
    }

    fn said(output: &str, errors: &str) -> Reply {
        Reply { output: output.to_string(), errors: errors.to_string() }
    }

    fn call(kind: AgentKind, prompt: &str) -> Call<'_> {
        Call { ticket: 1, kind, model: None, prompt }
    }

    #[test]
    fn the_answer_of_claude_is_the_result_field_and_an_error_is_a_failure_with_its_text() {
        let answer = |output: &str| result_answer(AgentKind::Claude, &said(output, ""));
        assert_eq!(
            answer(r#"{"type":"result","is_error":false,"result":"{\"action\":\"answer\"}","cost":1}"#),
            Ok(r#"{"action":"answer"}"#.to_string())
        );
        assert_eq!(
            answer(r#"{"type":"result","is_error":true,"result":"Invalid API key · Please run /login"}"#),
            Err("claude: Invalid API key · Please run /login".to_string())
        );
        assert_eq!(answer(r#"{"is_error":false}"#), Err("claude gave no answer".to_string()));
        assert_eq!(answer(r#"{"result":"  "}"#), Err("claude gave no answer".to_string()));
        assert_eq!(answer(""), Err("claude gave no answer".to_string()));
        assert_eq!(answer("command not found:\n claude"), Err("claude: command not found: claude".to_string()));
        let long = answer(&"x".repeat(MAX_PROBLEM_CHARS * 3)).unwrap_err();
        assert_eq!(long.chars().count(), "claude: ".len() + MAX_PROBLEM_CHARS);
    }

    #[test]
    fn an_engine_that_says_nothing_useful_is_explained_by_the_last_line_it_complained() {
        let complaint = "warning: old config\n\nerror: not logged in\n\n";
        assert_eq!(
            answer_of(AgentKind::Codex, &said(" \n", complaint)),
            Err("codex: error: not logged in".to_string())
        );
        assert_eq!(
            result_answer(AgentKind::Claude, &said("", complaint)),
            Err("claude: error: not logged in".to_string())
        );
        assert_eq!(
            result_answer(AgentKind::Claude, &said("Segmentation fault", complaint)),
            Err("claude: error: not logged in".to_string())
        );
        assert_eq!(
            result_answer(AgentKind::Claude, &said(r#"{"is_error":true}"#, complaint)),
            Err("claude: error: not logged in".to_string())
        );
        let refused = said(r#"{"is_error":true,"result":"Please run /login"}"#, complaint);
        assert_eq!(
            result_answer(AgentKind::Claude, &refused),
            Err("claude: Please run /login".to_string()),
            "its own words come first"
        );
        let fine = said(r#"{"result":"ok"}"#, complaint);
        assert_eq!(
            result_answer(AgentKind::Claude, &fine),
            Ok("ok".to_string()),
            "a complaint next to an answer is not a failure"
        );
        assert_eq!(answer_of(AgentKind::Codex, &said("ok", complaint)), Ok("ok".to_string()));
    }

    #[test]
    fn the_answer_of_codex_is_what_it_printed() {
        let printed = said("{\"action\":\"escalate\"}\n", "");
        assert_eq!(answer_of(AgentKind::Codex, &printed), Ok("{\"action\":\"escalate\"}\n".to_string()));
        assert_eq!(answer_of(AgentKind::Codex, &said(" \n", "")), Err("codex gave no answer".to_string()));
        assert_eq!(answer_of(AgentKind::Claude, &said(r#"{"result":"ok"}"#, "")), Ok("ok".to_string()));
    }

    fn fake_claude(dir: &std::path::Path, name: &str, body_sh: &str, body_ps1: &str) -> Config {
        std::fs::create_dir_all(dir).unwrap();
        if cfg!(windows) {
            let script = dir.join(format!("{name}.ps1"));
            std::fs::write(&script, body_ps1).unwrap();
            let args = ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].map(String::from);
            Config {
                claude_command: "powershell.exe".into(),
                claude_args: [args.to_vec(), vec![script.to_string_lossy().into_owned()]].concat(),
                ..Config::default()
            }
        } else {
            let script = dir.join(format!("{name}.sh"));
            std::fs::write(&script, body_sh).unwrap();
            Config {
                claude_command: "/bin/sh".into(),
                claude_args: vec![script.to_string_lossy().into_owned()],
                ..Config::default()
            }
        }
    }

    #[test]
    fn grok_reads_the_prompt_from_a_private_file_that_is_gone_after_it_answers_and_kimi_from_its_argument() {
        let dir = std::env::temp_dir().join(format!("orkai-octo-engine-file-{}-{}", std::process::id(), now_ms()));
        let home = Home::at(dir.join("home"));
        let reads_file = fake_claude(
            &dir,
            "grok",
            "while [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"--prompt-file\" ]; then cat \"$2\"; fi; shift; done\n",
            "for ($i = 0; $i -lt $args.Count; $i++) { if ($args[$i] -eq '--prompt-file') { [Console]::Out.Write([IO.File]::ReadAllText($args[$i + 1])) } }\n",
        );
        let grok = Config {
            grok_command: reads_file.claude_command.clone(),
            grok_args: reads_file.claude_args.clone(),
            ..Config::default()
        };
        let engine = CliEngine::with_timeout(&home, Duration::from_secs(60));
        let prompt = "judge this\nwith \"quotes\" & more";
        assert_eq!(engine.ask(&grok, &call(AgentKind::Grok, prompt)), Ok(prompt.to_string()));
        assert_eq!(engine.ask(&grok, &call(AgentKind::Grok, "again")), Ok("again".to_string()));
        let prompts = home.orchestrator_dir().join("prompts");
        assert!(prompts.is_dir());
        assert_eq!(std::fs::read_dir(&prompts).unwrap().count(), 0, "no prompt is left behind");

        let reads_argument = fake_claude(
            &dir,
            "kimi",
            "for arg in \"$@\"; do last=\"$arg\"; done\n[ \"$last\" = \"--prompt=what now\" ] && printf '{\"role\":\"assistant\",\"content\":\"heard\"}'\n",
            "if ($args[-1] -eq '--prompt=what now') { [Console]::Out.Write('{\"role\":\"assistant\",\"content\":\"heard\"}') }\n",
        );
        let kimi = Config {
            kimi_command: reads_argument.claude_command.clone(),
            kimi_args: reads_argument.claude_args.clone(),
            ..Config::default()
        };
        assert_eq!(engine.ask(&kimi, &call(AgentKind::Kimi, "what now")), Ok("heard".to_string()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_cli_engine_feeds_the_prompt_on_stdin_in_an_empty_folder_without_the_agent_id() {
        let dir = std::env::temp_dir().join(format!("orkai-octo-engine-{}-{}", std::process::id(), now_ms()));
        let home = Home::at(dir.join("home"));
        let config = fake_claude(
            &dir,
            "echo",
            r#"prompt=$(cat)
printf '{"is_error":false,"result":"%s|%s|%s"}' "$prompt" "$(basename "$PWD")" "${ORKAI_AGENT_ID:-unset}"
"#,
            r#"$prompt = [Console]::In.ReadToEnd()
$id = if ($env:ORKAI_AGENT_ID) { $env:ORKAI_AGENT_ID } else { 'unset' }
[Console]::Out.Write('{"is_error":false,"result":"' + $prompt + '|' + (Split-Path -Leaf $PWD.Path) + '|' + $id + '"}')
"#,
        );
        let engine = CliEngine::with_timeout(&home, Duration::from_secs(60));
        let answer = engine.ask(&config, &call(AgentKind::Claude, "what now")).unwrap();
        assert_eq!(answer, "what now|work|unset");
        assert_eq!(engine.running(), 0, "a finished engine is forgotten");
        assert!(home.orchestrator_dir().join("work").is_dir());
        assert_eq!(
            std::fs::read_dir(home.orchestrator_dir().join("work")).unwrap().count(),
            0,
            "the folder stays empty"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_engine_that_is_missing_hangs_or_fails_is_a_failure_with_a_reason() {
        let dir = std::env::temp_dir().join(format!("orkai-octo-engine-bad-{}-{}", std::process::id(), now_ms()));
        let home = Home::at(dir.join("home"));
        let missing = Config { claude_command: "orkai-definitely-missing-engine".into(), ..Config::default() };
        let engine = CliEngine::with_timeout(&home, Duration::from_millis(1_500));
        let not_found = engine.ask(&missing, &call(AgentKind::Claude, "hi")).unwrap_err();
        assert!(not_found.contains("orkai-definitely-missing-engine"), "{not_found}");

        let hung = fake_claude(&dir, "hang", "sleep 60\n", "Start-Sleep -Seconds 60\n");
        let started = std::time::Instant::now();
        assert_eq!(engine.ask(&hung, &call(AgentKind::Claude, "hi")), Err("claude did not answer".to_string()));
        assert!(started.elapsed() < Duration::from_secs(30), "a hung engine is killed");

        let signed_out = fake_claude(
            &dir,
            "signed-out",
            "cat > /dev/null\nprintf '{\"is_error\":true,\"result\":\"Please run /login\"}'\n",
            "[void][Console]::In.ReadToEnd()\n[Console]::Out.Write('{\"is_error\":true,\"result\":\"Please run /login\"}')\n",
        );
        let slow = CliEngine::with_timeout(&home, Duration::from_secs(60));
        assert_eq!(slow.ask(&signed_out, &call(AgentKind::Claude, "hi")), Err("claude: Please run /login".to_string()));
        assert!(slow.ask(&signed_out, &call(AgentKind::Terminal, "hi")).is_err());

        let complains = fake_claude(
            &dir,
            "complains",
            "cat > /dev/null\necho 'error: not logged in' 1>&2\n",
            "[void][Console]::In.ReadToEnd()\n[Console]::Error.WriteLine('error: not logged in')\n",
        );
        assert_eq!(
            slow.ask(&complains, &call(AgentKind::Claude, "hi")),
            Err("claude: error: not logged in".to_string()),
            "what it wrote to its error output explains the silence"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_running_engine_is_stopped_by_its_ticket_or_all_at_once() {
        let dir = std::env::temp_dir().join(format!("orkai-octo-engine-stop-{}-{}", std::process::id(), now_ms()));
        let home = Home::at(dir.join("home"));
        let hung = fake_claude(&dir, "hang", "sleep 60\n", "Start-Sleep -Seconds 60\n");
        let engine = std::sync::Arc::new(CliEngine::with_timeout(&home, Duration::from_secs(50)));
        let ask = |ticket: u64| {
            let (engine, config) = (std::sync::Arc::clone(&engine), hung.clone());
            std::thread::spawn(move || engine.ask(&config, &Call { ticket, ..call(AgentKind::Claude, "hi") }))
        };
        let wait_for = |count: usize| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while engine.running() != count && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(engine.running(), count);
        };
        let started = std::time::Instant::now();
        let (first, second, third) = (ask(7), ask(8), ask(9));
        wait_for(3);
        engine.stop(99);
        assert_eq!(engine.running(), 3, "a ticket that is not running stops nothing now");
        let early = std::time::Instant::now();
        let refused = engine.ask(&hung, &Call { ticket: 99, ..call(AgentKind::Claude, "hi") });
        assert_eq!(refused, Err(STOPPED.to_string()), "but an engine asked for it afterwards never starts");
        assert!(early.elapsed() < Duration::from_secs(5));
        assert_eq!(engine.running(), 3);
        for ticket in 100..100 + 2 * MAX_REMEMBERED_STOPS as u64 {
            engine.stop(ticket);
        }
        assert_eq!(lock(&engine.stopped).len(), MAX_REMEMBERED_STOPS, "the stops it remembers are bounded");
        engine.stop(7);
        assert!(first.join().unwrap().is_err());
        wait_for(2);
        engine.stop_all();
        assert!(second.join().unwrap().is_err() && third.join().unwrap().is_err());
        wait_for(0);
        assert!(started.elapsed() < Duration::from_secs(40), "none of them waited for its timeout");
        let closed = engine.ask(&hung, &call(AgentKind::Claude, "hi"));
        assert_eq!(closed, Err(STOPPED.to_string()), "after everything was stopped no engine starts again");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
