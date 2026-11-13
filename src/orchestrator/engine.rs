use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Deserialize;

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
        _ => None,
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
struct ClaudeOutput {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: bool,
}

fn problem(kind: AgentKind, said: &str) -> String {
    let said = one_line(said, MAX_PROBLEM_CHARS);
    if said.is_empty() { format!("{} gave no answer", kind.label()) } else { format!("{}: {said}", kind.label()) }
}

fn last_line(text: &str) -> &str {
    text.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default()
}

pub fn claude_answer(reply: &Reply) -> Result<String, String> {
    let complaint = last_line(&reply.errors);
    let Ok(parsed) = serde_json::from_str::<ClaudeOutput>(reply.output.trim()) else {
        let said = if complaint.is_empty() { reply.output.as_str() } else { complaint };
        return Err(problem(AgentKind::Claude, said));
    };
    match parsed.result.filter(|result| !result.trim().is_empty()) {
        Some(result) if !parsed.is_error => Ok(result),
        Some(refusal) => Err(problem(AgentKind::Claude, &refusal)),
        None => Err(problem(AgentKind::Claude, complaint)),
    }
}

pub fn answer_of(kind: AgentKind, reply: &Reply) -> Result<String, String> {
    match kind {
        AgentKind::Claude => claude_answer(reply),
        _ if reply.output.trim().is_empty() => Err(problem(kind, last_line(&reply.errors))),
        _ => Ok(reply.output.clone()),
    }
}

pub struct CliEngine {
    work: PathBuf,
    timeout: Duration,
    running: Mutex<Vec<(u64, u32)>>,
    stopped: Mutex<VecDeque<u64>>,
    closing: AtomicBool,
}

impl CliEngine {
    pub fn new(home: &Home) -> Self {
        Self::with_timeout(home, ENGINE_TIMEOUT)
    }

    pub fn with_timeout(home: &Home, timeout: Duration) -> Self {
        Self {
            work: home.orchestrator_dir().join(WORK_DIR),
            timeout,
            running: Mutex::new(Vec::new()),
            stopped: Mutex::new(VecDeque::new()),
            closing: AtomicBool::new(false),
        }
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
        let (command, args) = command_line(config, call.kind, call.model)?;
        let program = launch::resolve_program(&command).map_err(|err| err.to_string())?;
        let (program, args) = launch::wrap_for_platform(program, args).map_err(|err| err.to_string())?;
        create_private_dir(&self.work).map_err(|err| err.to_string())?;
        let mut command = Command::new(program);
        command.args(args).current_dir(&self.work).env_remove(launch::AGENT_ID_ENV);
        let mut child = None;
        let input = call.prompt.as_bytes().to_vec();
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
        assert_eq!(engine_args(AgentKind::Kimi, None), None);
        assert_eq!(engine_args(AgentKind::Terminal, None), None);
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
        let refused = command_line(&config, AgentKind::Kimi, None).unwrap_err();
        assert!(refused.contains("kimi"), "{refused}");
        assert!(command_line(&config, AgentKind::Terminal, None).is_err());
    }

    fn said(output: &str, errors: &str) -> Reply {
        Reply { output: output.to_string(), errors: errors.to_string() }
    }

    fn call(kind: AgentKind, prompt: &str) -> Call<'_> {
        Call { ticket: 1, kind, model: None, prompt }
    }

    #[test]
    fn the_answer_of_claude_is_the_result_field_and_an_error_is_a_failure_with_its_text() {
        let answer = |output: &str| claude_answer(&said(output, ""));
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
        assert_eq!(claude_answer(&said("", complaint)), Err("claude: error: not logged in".to_string()));
        assert_eq!(
            claude_answer(&said("Segmentation fault", complaint)),
            Err("claude: error: not logged in".to_string())
        );
        assert_eq!(
            claude_answer(&said(r#"{"is_error":true}"#, complaint)),
            Err("claude: error: not logged in".to_string())
        );
        let refused = said(r#"{"is_error":true,"result":"Please run /login"}"#, complaint);
        assert_eq!(claude_answer(&refused), Err("claude: Please run /login".to_string()), "its own words come first");
        let fine = said(r#"{"result":"ok"}"#, complaint);
        assert_eq!(claude_answer(&fine), Ok("ok".to_string()), "a complaint next to an answer is not a failure");
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
        assert!(slow.ask(&signed_out, &call(AgentKind::Kimi, "hi")).is_err());

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
