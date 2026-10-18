use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::launch;
use crate::model::AgentId;
use crate::paths::Home;
use crate::util::{lock, now_ms};

pub const HOOK_COMMAND: &str = "__hook";
pub const CLAUDE_SOURCE: &str = "claude";
pub const CODEX_SOURCE: &str = "codex";
pub const MIN_CLAUDE_VERSION: (u32, u32, u32) = (2, 1, 78);
pub const CLAUDE_EVENTS: [&str; 6] =
    ["UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "StopFailure"];
pub const HOOK_TIMEOUT_SECS: u64 = 10;
pub const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_EVENTS_PER_POLL: usize = 64;
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const VERSION_POLL: Duration = Duration::from_millis(20);
const QUESTION_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];
const EVENT_SUFFIX: &str = ".json";
const GIT_BASH_NAMES: [&str; 4] = ["bash.exe", "sh.exe", "bash", "sh"];
const GIT_BASH_LOCATIONS: [&str; 2] =
    [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"];

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HookSignal {
    Busy,
    Waiting,
    Finished,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookEvent {
    pub signal: HookSignal,
    pub at_ms: u64,
}

#[derive(Deserialize)]
struct ClaudeInput {
    hook_event_name: String,
    #[serde(default)]
    tool_name: Option<String>,
}

#[derive(Deserialize)]
struct CodexNotification {
    #[serde(rename = "type")]
    kind: String,
}

pub fn claude_signal(input: &[u8]) -> Option<HookSignal> {
    let input: ClaudeInput = serde_json::from_slice(input).ok()?;
    let asks = input.tool_name.as_deref().is_some_and(|tool| QUESTION_TOOLS.contains(&tool));
    match input.hook_event_name.as_str() {
        "PreToolUse" if asks => Some(HookSignal::Waiting),
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => Some(HookSignal::Busy),
        "PermissionRequest" => Some(HookSignal::Waiting),
        "Stop" | "StopFailure" => Some(HookSignal::Finished),
        _ => None,
    }
}

pub fn codex_signal(payload: &str) -> Option<HookSignal> {
    let notification: CodexNotification = serde_json::from_str(payload).ok()?;
    (notification.kind == "agent-turn-complete").then_some(HookSignal::Finished)
}

fn read_payload(mut input: impl Read) -> Vec<u8> {
    let mut payload = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match input.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if payload.len() + n <= MAX_PAYLOAD_BYTES {
                    payload.extend_from_slice(&chunk[..n]);
                } else {
                    payload.clear();
                    payload.shrink_to_fit();
                    io::copy(&mut input, &mut io::sink()).ok();
                    return Vec::new();
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    payload
}

pub fn signal_from(source: &str, payload: Option<&str>, stdin: impl Read) -> Option<HookSignal> {
    match source {
        CLAUDE_SOURCE => claude_signal(&read_payload(stdin)),
        CODEX_SOURCE => codex_signal(payload?),
        _ => None,
    }
}

pub fn write_event(home: &Home, id: AgentId, event: HookEvent) -> io::Result<()> {
    if !home.agent_dir(id).is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("agent {id} not found")));
    }
    let dir = home.hooks_dir(id);
    fs::create_dir_all(&dir)?;
    let name = format!("{:016}-{:010}{EVENT_SUFFIX}", event.at_ms, std::process::id());
    fsutil::write_json_atomic(&dir.join(name), &event)
}

pub fn record(home: &Home, id: AgentId, source: &str, payload: Option<&str>) -> io::Result<()> {
    let at_ms = now_ms();
    match signal_from(source, payload, io::stdin().lock()) {
        Some(signal) => write_event(home, id, HookEvent { signal, at_ms }),
        None => Ok(()),
    }
}

pub struct HookInbox {
    dir: PathBuf,
    last_at: u64,
}

impl HookInbox {
    pub fn open(dir: PathBuf) -> Self {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
        Self { dir, last_at: 0 }
    }

    pub fn take(&mut self) -> Vec<HookEvent> {
        let Ok(entries) = fs::read_dir(&self.dir) else { return Vec::new() };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| name.ends_with(EVENT_SUFFIX) && !name.starts_with('.'))
            .collect();
        names.sort_unstable();
        names.truncate(MAX_EVENTS_PER_POLL);
        let mut events = Vec::new();
        for name in names {
            let path = self.dir.join(&name);
            let event = fsutil::read_json::<HookEvent>(&path).ok();
            let _ = fs::remove_file(&path);
            if let Some(event) = event.filter(|event| event.at_ms >= self.last_at) {
                self.last_at = event.at_ms;
                events.push(event);
            }
        }
        events
    }
}

pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

pub fn claude_hook_command(exe: &Path, id: AgentId) -> String {
    let program = exe.to_string_lossy().replace('\\', "/");
    format!("{} {HOOK_COMMAND} {CLAUDE_SOURCE} {id}", shell_quote(&program))
}

pub fn claude_settings(command: &str) -> serde_json::Value {
    let handler = serde_json::json!([{
        "hooks": [{ "type": "command", "command": command, "async": true, "timeout": HOOK_TIMEOUT_SECS }]
    }]);
    let hooks: serde_json::Map<String, serde_json::Value> =
        CLAUDE_EVENTS.iter().map(|event| (event.to_string(), handler.clone())).collect();
    serde_json::json!({ "hooks": hooks })
}

fn toml_string(text: &str) -> String {
    if text.contains(['\'', '\n', '\r']) {
        serde_json::to_string(text).unwrap_or_default()
    } else {
        format!("'{text}'")
    }
}

pub fn codex_notify_override(exe: &Path, id: AgentId) -> String {
    let argv = [exe.to_string_lossy().into_owned(), HOOK_COMMAND.into(), CODEX_SOURCE.into(), id.to_string()];
    let items: Vec<String> = argv.iter().map(|item| toml_string(item)).collect();
    format!("notify=[{}]", items.join(","))
}

pub fn codex_has_own_notify(codex_home: &Path) -> bool {
    let Ok(text) = fs::read_to_string(codex_home.join("config.toml")) else { return false };
    text.lines().any(|line| {
        let line = line.trim_start();
        line.strip_prefix("notify").is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let token = text.split_whitespace().next()?;
    let mut parts = token.split('.').map(|part| part.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

fn program_version(program: &Path) -> Option<(u32, u32, u32)> {
    let (program, args) = launch::wrap_for_platform(program.to_path_buf(), vec!["--version".to_string()]).ok()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).ok().map(|_| text)
    });
    let deadline = Instant::now() + VERSION_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(VERSION_POLL),
            _ => {
                launch::kill_process_tree(child.id());
                let _ = child.wait();
                return None;
            }
        }
    }
    parse_version(&reader.join().ok()??)
}

type VersionKey = (Option<SystemTime>, u64);

static CLAUDE_VERSIONS: OnceLock<Mutex<HashMap<PathBuf, (VersionKey, bool)>>> = OnceLock::new();

pub fn claude_supports_hooks(program: &Path) -> bool {
    let key = fs::metadata(program).map(|m| (m.modified().ok(), m.len())).unwrap_or((None, 0));
    let cache = CLAUDE_VERSIONS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((cached, supported)) = lock(cache).get(program)
        && *cached == key
    {
        return *supported;
    }
    let supported = program_version(program).is_some_and(|version| version >= MIN_CLAUDE_VERSION);
    lock(cache).insert(program.to_path_buf(), (key, supported));
    supported
}

pub fn git_bash_available() -> bool {
    if !cfg!(windows) {
        return true;
    }
    if let Some(path) = std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").map(PathBuf::from) {
        let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if GIT_BASH_NAMES.contains(&name.as_str()) && path.exists() {
            return true;
        }
    }
    GIT_BASH_LOCATIONS.iter().any(|path| Path::new(path).exists())
        || which::which("git")
            .ok()
            .and_then(|git| Some(git.parent()?.parent()?.join("bin").join("bash.exe")))
            .is_some_and(|bash| bash.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orkai-hooks-{tag}-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn claude(event: &str, tool: Option<&str>) -> Option<HookSignal> {
        let mut input = serde_json::json!({
            "session_id": "5bd0ccb0-0d09-47ca-9524-6eb740341fa9",
            "hook_event_name": event,
            "prompt": "secret task text",
            "tool_input": { "command": "rm -rf build" }
        });
        if let Some(tool) = tool {
            input["tool_name"] = tool.into();
        }
        claude_signal(input.to_string().as_bytes())
    }

    #[test]
    fn claude_events_map_to_agent_signals() {
        assert_eq!(claude("UserPromptSubmit", None), Some(HookSignal::Busy));
        assert_eq!(claude("PreToolUse", Some("Bash")), Some(HookSignal::Busy));
        assert_eq!(claude("PostToolUse", Some("Bash")), Some(HookSignal::Busy));
        assert_eq!(claude("PreToolUse", Some("AskUserQuestion")), Some(HookSignal::Waiting));
        assert_eq!(claude("PreToolUse", Some("ExitPlanMode")), Some(HookSignal::Waiting));
        assert_eq!(claude("PostToolUse", Some("AskUserQuestion")), Some(HookSignal::Busy));
        assert_eq!(claude("PermissionRequest", Some("Bash")), Some(HookSignal::Waiting));
        assert_eq!(claude("Stop", None), Some(HookSignal::Finished));
        assert_eq!(claude("StopFailure", None), Some(HookSignal::Finished));
        assert_eq!(claude("SessionStart", None), None);
        assert_eq!(claude_signal(b"not json"), None);
        assert_eq!(claude_signal(b"{}"), None);
    }

    #[test]
    fn codex_turn_completion_is_a_finished_turn() {
        let payload = r#"{"type":"agent-turn-complete","thread-id":"t","turn-id":"1","cwd":"/x","input-messages":["secret"],"last-assistant-message":"done"}"#;
        assert_eq!(codex_signal(payload), Some(HookSignal::Finished));
        assert_eq!(codex_signal(r#"{"type":"something-else"}"#), None);
        assert_eq!(codex_signal("garbage"), None);
        assert_eq!(signal_from(CODEX_SOURCE, Some(payload), io::empty()), Some(HookSignal::Finished));
        assert_eq!(signal_from(CODEX_SOURCE, None, io::empty()), None);
        assert_eq!(signal_from("other", Some(payload), io::empty()), None);
    }

    #[test]
    fn claude_payload_is_read_from_stdin_and_oversized_input_is_drained() {
        let stop = br#"{"hook_event_name":"Stop"}"#;
        assert_eq!(signal_from(CLAUDE_SOURCE, None, &stop[..]), Some(HookSignal::Finished));
        let mut huge = br#"{"hook_event_name":"PostToolUse","tool_response":""#.to_vec();
        huge.extend(std::iter::repeat_n(b'x', MAX_PAYLOAD_BYTES));
        huge.extend_from_slice(br#""}"#);
        let mut reader = &huge[..];
        assert_eq!(signal_from(CLAUDE_SOURCE, None, &mut reader), None);
        assert!(reader.is_empty(), "the whole payload must be consumed so the agent never sees a broken pipe");
    }

    #[test]
    fn events_are_written_only_for_existing_agents_and_read_once_in_order() {
        let dir = temp_dir("inbox");
        let home = Home::at(&dir);
        home.ensure().unwrap();
        let missing = write_event(&home, 9, HookEvent { signal: HookSignal::Busy, at_ms: 1 }).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        assert!(!home.hooks_dir(9).exists());

        fs::create_dir_all(home.agent_dir(3)).unwrap();
        fs::create_dir_all(home.hooks_dir(3)).unwrap();
        fs::write(home.hooks_dir(3).join("0000000000000001-0000000001.json"), b"{}").unwrap();
        let mut inbox = HookInbox::open(home.hooks_dir(3));
        assert!(inbox.take().is_empty(), "events left by a previous run are discarded");

        write_event(&home, 3, HookEvent { signal: HookSignal::Finished, at_ms: 2_000 }).unwrap();
        write_event(&home, 3, HookEvent { signal: HookSignal::Busy, at_ms: 1_000 }).unwrap();
        fs::write(home.hooks_dir(3).join(".partial.json.123.0.tmp"), b"{").unwrap();
        fs::write(home.hooks_dir(3).join("0000000000001500-0000000002.json"), b"broken").unwrap();
        assert_eq!(
            inbox.take(),
            vec![
                HookEvent { signal: HookSignal::Busy, at_ms: 1_000 },
                HookEvent { signal: HookSignal::Finished, at_ms: 2_000 }
            ]
        );
        assert!(inbox.take().is_empty());
        write_event(&home, 3, HookEvent { signal: HookSignal::Busy, at_ms: 1_200 }).unwrap();
        assert!(inbox.take().is_empty(), "an event older than the last one applied arrives too late");
        let left: Vec<_> = fs::read_dir(home.hooks_dir(3)).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left.len(), 1, "only the in-progress temporary file remains: {left:?}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_burst_of_events_is_read_in_bounded_batches() {
        let dir = temp_dir("burst");
        let home = Home::at(&dir);
        fs::create_dir_all(home.agent_dir(1)).unwrap();
        let mut inbox = HookInbox::open(home.hooks_dir(1));
        for at_ms in 0..(MAX_EVENTS_PER_POLL as u64 + 10) {
            let name = format!("{at_ms:016}-{:010}{EVENT_SUFFIX}", 1);
            let event = HookEvent { signal: HookSignal::Busy, at_ms };
            fs::create_dir_all(home.hooks_dir(1)).unwrap();
            fsutil::write_json_atomic(&home.hooks_dir(1).join(name), &event).unwrap();
        }
        assert_eq!(inbox.take().len(), MAX_EVENTS_PER_POLL);
        assert_eq!(inbox.take().len(), 10);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hook_command_is_quoted_for_posix_shells() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        let command = claude_hook_command(Path::new(r"C:\Users\O'Neil Smith\orkai\bin\orkai-holder.exe"), 12);
        assert_eq!(command, r"'C:/Users/O'\''Neil Smith/orkai/bin/orkai-holder.exe' __hook claude 12");
        let unix = claude_hook_command(Path::new("/home/me/.local/share/orkai/bin/orkai-holder"), 3);
        assert_eq!(unix, "'/home/me/.local/share/orkai/bin/orkai-holder' __hook claude 3");
    }

    #[test]
    fn claude_settings_register_one_async_hook_per_event() {
        let settings = claude_settings("'orkai' __hook claude 1");
        let hooks = settings["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), CLAUDE_EVENTS.len());
        for event in CLAUDE_EVENTS {
            let handler = &hooks[event][0]["hooks"][0];
            assert_eq!(handler["type"], "command");
            assert_eq!(handler["command"], "'orkai' __hook claude 1");
            assert_eq!(handler["async"], true);
            assert_eq!(handler["timeout"], HOOK_TIMEOUT_SECS);
            assert!(hooks[event][0].get("matcher").is_none());
        }
    }

    #[test]
    fn codex_notify_override_is_a_toml_array() {
        assert_eq!(
            codex_notify_override(Path::new(r"C:\Users\me\orkai\bin\orkai-holder.exe"), 4),
            r"notify=['C:\Users\me\orkai\bin\orkai-holder.exe','__hook','codex','4']"
        );
        assert_eq!(
            codex_notify_override(Path::new("/home/o'neil/orkai-holder"), 4),
            r#"notify=["/home/o'neil/orkai-holder",'__hook','codex','4']"#
        );
    }

    #[test]
    fn a_notify_already_configured_for_codex_is_respected() {
        let dir = temp_dir("codex-notify");
        assert!(!codex_has_own_notify(&dir));
        fs::write(dir.join("config.toml"), "model = \"gpt-5\"\nnotifications = true\n# notify = [\"x\"]\n").unwrap();
        assert!(!codex_has_own_notify(&dir));
        fs::write(dir.join("config.toml"), "model = \"gpt-5\"\n  notify = [\"notify-send\", \"Codex\"]\n").unwrap();
        assert!(codex_has_own_notify(&dir));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn versions_are_parsed_from_the_version_banner() {
        assert_eq!(parse_version("2.1.302 (Claude Code)"), Some((2, 1, 302)));
        assert_eq!(parse_version("2.1.78\n"), Some((2, 1, 78)));
        assert_eq!(parse_version("Claude Code"), None);
        assert_eq!(parse_version(""), None);
        assert!(parse_version("2.1.77").unwrap() < MIN_CLAUDE_VERSION);
        assert!(parse_version("2.2.0").unwrap() >= MIN_CLAUDE_VERSION);
    }

    fn file_stem(version: &str) -> String {
        version.chars().filter(char::is_ascii_alphanumeric).collect()
    }

    fn fake_claude(dir: &Path, version: &str) -> PathBuf {
        if cfg!(windows) {
            let script = dir.join(format!("claude-{}.cmd", file_stem(version)));
            fs::write(&script, format!("@echo {version}\r\n")).unwrap();
            script
        } else {
            let script = dir.join(format!("claude-{}", file_stem(version)));
            fs::write(&script, format!("#!/bin/sh\necho '{version}'\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            }
            script
        }
    }

    #[test]
    fn only_claude_versions_that_know_every_hook_event_get_hooks() {
        let dir = temp_dir("versions");
        assert!(claude_supports_hooks(&fake_claude(&dir, "2.1.302 (Claude Code)")));
        assert!(!claude_supports_hooks(&fake_claude(&dir, "2.1.77 (Claude Code)")));
        assert!(!claude_supports_hooks(&fake_claude(&dir, "unknown")));
        assert!(!claude_supports_hooks(&dir.join("missing-claude")));
        fs::remove_dir_all(dir).unwrap();
    }
}
