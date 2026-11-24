use crate::holder::nested::{program_name, program_of};
use crate::launch::{bypass_flag, chosen_session_args, resume_args};
use crate::model::{AgentKind, SavedNested};

const MAX_TYPED_CHARS: usize = 4_096;
const RUNTIMES: [&str; 7] = ["node", "bun", "deno", "python", "python3", "py", "tsx"];
const END_OF_OPTIONS: &str = "--";
const POWERSHELL_QUOTES: [char; 5] = ['"', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];
const POWERSHELL_CALL: &str = "& ";

struct Options {
    resume: &'static [&'static str],
    single: &'static [&'static str],
    variadic: &'static [&'static str],
}

const CLAUDE: Options = Options {
    resume: &["-r", "--resume", "-c", "--continue", "--session-id", "--fork-session", "--from-pr", "--teleport"],
    single: &[
        "--agent",
        "--agents",
        "--append-system-prompt",
        "--append-system-prompt-file",
        "--autocompact",
        "--debug-file",
        "--effort",
        "--environment",
        "--fallback-model",
        "--input-format",
        "--json-schema",
        "--max-budget-usd",
        "--max-turns",
        "--model",
        "-n",
        "--name",
        "--output-format",
        "--permission-mode",
        "--permission-prompt-tool",
        "--permission-prompts",
        "--plugin-dir",
        "--plugin-url",
        "--remote-control-session-name-prefix",
        "--setting-sources",
        "--settings",
        "--system-prompt",
        "--system-prompt-file",
        "--system-prompt-snapshot",
    ],
    variadic: &[
        "--add-dir",
        "--allowedTools",
        "--allowed-tools",
        "--betas",
        "--disallowedTools",
        "--disallowed-tools",
        "--file",
        "--mcp-config",
        "--tools",
    ],
};

const CODEX: Options = Options {
    resume: &["--last", "--all", "--include-non-interactive"],
    single: &[
        "-c",
        "--config",
        "--enable",
        "--disable",
        "--remote",
        "--remote-auth-token-env",
        "-m",
        "--model",
        "--local-provider",
        "-p",
        "--profile",
        "-s",
        "--sandbox",
        "-C",
        "--cd",
        "--add-dir",
        "-a",
        "--ask-for-approval",
    ],
    variadic: &["-i", "--image"],
};

const LISTED: Options = Options {
    resume: &["-s", "--session", "-c", "--continue", "--fork", "--prompt"],
    single: &["-m", "--model", "--agent"],
    variadic: &[],
};

const QWEN: Options = Options {
    resume: &["-r", "--resume", "-c", "--continue", "--session-id", "-p", "--prompt", "-i", "--prompt-interactive"],
    single: &["-m", "--model", "--approval-mode"],
    variadic: &[],
};

const ANTIGRAVITY: Options = Options {
    resume: &["-c", "--continue", "--conversation", "-p", "--print", "--prompt", "-i", "--prompt-interactive"],
    single: &["--agent", "--effort", "--log-file", "--mode", "--model", "--project", "--settings"],
    variadic: &[],
};

const PI: Options = Options {
    resume: &["-c", "--continue", "-r", "--resume", "--session", "--session-id", "--fork"],
    single: &[
        "--provider",
        "--model",
        "--api-key",
        "--system-prompt",
        "--append-system-prompt",
        "--mode",
        "--session-dir",
        "-n",
        "--name",
        "--models",
        "-t",
        "--tools",
        "-xt",
        "--exclude-tools",
        "--thinking",
        "-e",
        "--extension",
        "--skill",
        "--prompt-template",
        "--theme",
        "--use-theme",
        "--tui-mode",
    ],
    variadic: &[],
};

const OTHER: Options = Options {
    resume: &["--resume", "--continue", "--session", "--session-id", "--conversation", "--fork"],
    single: &["--model"],
    variadic: &[],
};

fn options(kind: AgentKind) -> &'static Options {
    match kind {
        AgentKind::Claude => &CLAUDE,
        AgentKind::Codex => &CODEX,
        AgentKind::Kilo | AgentKind::Opencode => &LISTED,
        AgentKind::Qwen => &QWEN,
        AgentKind::Antigravity => &ANTIGRAVITY,
        AgentKind::Pi => &PI,
        AgentKind::Grok | AgentKind::Kimi | AgentKind::Cursor | AgentKind::Terminal => &OTHER,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Nothing,
    One,
    Many,
    Maybe,
}

fn is_option(arg: &str) -> bool {
    arg.len() > 1 && arg.starts_with('-')
}

fn replayed_args(kind: AgentKind, args: &[String], has_session: bool) -> Vec<String> {
    let options = options(kind);
    let mut kept = Vec::with_capacity(args.len());
    let mut expect = Expect::Nothing;
    for arg in args {
        if arg == END_OF_OPTIONS {
            break;
        }
        if is_option(arg) {
            let (name, inline) = arg.split_once('=').map_or((arg.as_str(), false), |(name, _)| (name, true));
            if has_session && options.resume.contains(&name) {
                expect = Expect::Nothing;
                continue;
            }
            expect = if inline || bypass_flag(kind) == Some(name) {
                Expect::Nothing
            } else if options.single.contains(&name) {
                Expect::One
            } else if options.variadic.contains(&name) {
                Expect::Many
            } else {
                Expect::Maybe
            };
            kept.push(arg.clone());
            continue;
        }
        let keep = match expect {
            Expect::One | Expect::Many => true,
            Expect::Maybe => !arg.contains(char::is_whitespace),
            Expect::Nothing => false,
        };
        if keep {
            kept.push(arg.clone());
        }
        if expect != Expect::Many {
            expect = Expect::Nothing;
        }
    }
    kept
}

fn split_program(command: &[String]) -> (&[String], &[String]) {
    let Some(first) = command.first() else { return (&[], &[]) };
    if !RUNTIMES.contains(&program_name(first).as_str()) {
        return command.split_at(1);
    }
    match command.iter().skip(1).position(|arg| !is_option(arg)) {
        Some(script) => command.split_at(script + 2),
        None => command.split_at(1),
    }
}

fn session_args(kind: AgentKind, session: &str) -> Vec<String> {
    match kind {
        AgentKind::Claude => vec!["--resume".to_string(), session.to_string()],
        AgentKind::Codex => vec!["resume".to_string(), session.to_string()],
        AgentKind::Grok | AgentKind::Qwen | AgentKind::Pi => chosen_session_args(kind, session, true),
        AgentKind::Kilo | AgentKind::Opencode | AgentKind::Antigravity | AgentKind::Kimi | AgentKind::Cursor => {
            resume_args(kind, session)
        }
        AgentKind::Terminal => Vec::new(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Quoting {
    PowerShell,
    Cmd,
    Posix,
    Other,
}

fn shell_of(program: &str) -> Quoting {
    match program_name(program).as_str() {
        "pwsh" | "powershell" => Quoting::PowerShell,
        "cmd" => Quoting::Cmd,
        "sh" | "bash" | "zsh" | "dash" | "ksh" => Quoting::Posix,
        _ => Quoting::Other,
    }
}

fn is_bare(shell: Quoting, token: &str) -> bool {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '=' | '/' | '.' | ':');
    match shell {
        Quoting::PowerShell => {
            token.chars().all(|c| plain(c) || c == '\\') && !(token.starts_with('-') && token.contains(['.', ':']))
        }
        Quoting::Cmd => token.chars().all(|c| plain(c) || c == '\\'),
        Quoting::Posix => token.chars().all(|c| plain(c) || matches!(c, '+' | '@' | '%' | ',')),
        Quoting::Other => token.chars().all(plain),
    }
}

fn quoted(shell: Quoting, token: &str) -> Option<String> {
    if token.is_empty() || token.contains(char::is_control) {
        return None;
    }
    if is_bare(shell, token) {
        return Some(token.to_string());
    }
    match shell {
        Quoting::PowerShell => (!token.contains(POWERSHELL_QUOTES) && !token.ends_with('\\'))
            .then(|| format!("'{}'", token.replace('\'', "''"))),
        Quoting::Posix => Some(format!("'{}'", token.replace('\'', "'\\''"))),
        Quoting::Cmd | Quoting::Other => None,
    }
}

fn typed(shell: Quoting, tokens: &[String]) -> Option<String> {
    let words = tokens.iter().map(|token| quoted(shell, token)).collect::<Option<Vec<String>>>()?;
    let call = match (shell, tokens.first(), words.first()) {
        (Quoting::PowerShell, Some(token), Some(word)) if token != word => POWERSHELL_CALL,
        _ => "",
    };
    let line = format!("{call}{}", words.join(" "));
    (!words.is_empty() && line.chars().count() <= MAX_TYPED_CHARS).then_some(line)
}

pub fn command(saved: &SavedNested, session: Option<&str>, shell: &str) -> Option<String> {
    let kind = saved.agent.kind;
    let shell = shell_of(shell);
    let resume = session.map(|session| session_args(kind, session)).unwrap_or_default();
    let (program, args) = split_program(&saved.command);
    let replayed: Vec<String> = program
        .iter()
        .cloned()
        .chain(resume.iter().cloned())
        .chain(replayed_args(kind, args, session.is_some()))
        .collect();
    let exact = (!program.is_empty()).then(|| typed(shell, &replayed)).flatten();
    exact.or_else(|| {
        let plain: Vec<String> = program_of(kind).map(str::to_string).into_iter().chain(resume).collect();
        typed(shell, &plain)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NestedAgent;

    const SESSION: &str = "6937ab64-8ff7-4c75-bfa9-cf3bd46c2fa0";

    fn saved(kind: AgentKind, command: &[&str]) -> SavedNested {
        SavedNested {
            agent: NestedAgent { kind, pid: 7, conversation_id: None, cwd: None, since_ms: 1 },
            command: command.iter().map(|arg| arg.to_string()).collect(),
        }
    }

    fn resumed(kind: AgentKind, command: &[&str], shell: &str) -> String {
        super::command(&saved(kind, command), Some(SESSION), shell).unwrap()
    }

    #[test]
    fn claude_comes_back_on_its_conversation_with_the_parameters_it_was_started_with() {
        let typed = resumed(
            AgentKind::Claude,
            &[r"C:\Users\me\.local\bin\claude.exe", "--dangerously-skip-permissions", "--model", "opus"],
            "pwsh",
        );
        assert_eq!(
            typed,
            format!(
                r"C:\Users\me\.local\bin\claude.exe --resume {SESSION} --dangerously-skip-permissions --model opus"
            )
        );
        assert_eq!(resumed(AgentKind::Claude, &["claude"], "/bin/zsh"), format!("claude --resume {SESSION}"));
    }

    #[test]
    fn the_first_prompt_and_the_old_way_of_picking_a_conversation_are_not_typed_again() {
        let typed = resumed(
            AgentKind::Claude,
            &[
                "claude",
                "--dangerously-skip-permissions",
                "fix",
                "--resume",
                "0000",
                "--model",
                "opus",
                "--add-dir",
                "../api",
                "../web",
                "-c",
                "--session-id=abc",
                "--verbose",
                "refactor the whole login",
                "--",
                "--not-an-option",
            ],
            "bash",
        );
        assert_eq!(
            typed,
            format!(
                "claude --resume {SESSION} --dangerously-skip-permissions --model opus --add-dir ../api ../web --verbose"
            )
        );
    }

    #[test]
    fn without_a_conversation_to_resume_the_command_keeps_its_own_way_of_picking_one() {
        let typed = super::command(
            &saved(AgentKind::Pi, &["pi", "--session-id", "mine", "--approve", "do", "it"]),
            None,
            "bash",
        );
        assert_eq!(typed.as_deref(), Some("pi --session-id mine --approve"));
        let fresh =
            super::command(&saved(AgentKind::Claude, &["claude", "--model", "opus", "hello there"]), None, "sh");
        assert_eq!(fresh.as_deref(), Some("claude --model opus"));
    }

    #[test]
    fn every_agent_resumes_with_its_own_option() {
        for (kind, program, expected) in [
            (AgentKind::Codex, "codex", format!("codex resume {SESSION} --search -m gpt-6")),
            (AgentKind::Kilo, "kilo", format!("kilo --session {SESSION} --search -m gpt-6")),
            (AgentKind::Opencode, "opencode", format!("opencode --session {SESSION} --search -m gpt-6")),
            (AgentKind::Antigravity, "agy", format!("agy --conversation={SESSION} --search -m gpt-6")),
            (AgentKind::Grok, "grok", format!("grok --resume={SESSION} --search -m gpt-6")),
            (AgentKind::Qwen, "qwen", format!("qwen --resume {SESSION} --search -m gpt-6")),
            (AgentKind::Kimi, "kimi", format!("kimi --session {SESSION} --search -m gpt-6")),
            (AgentKind::Pi, "pi", format!("pi --session-id {SESSION} --search -m gpt-6")),
            (AgentKind::Cursor, "cursor-agent", format!("cursor-agent --resume {SESSION} --search -m gpt-6")),
        ] {
            assert_eq!(resumed(kind, &[program, "--search", "-m", "gpt-6"], "bash"), expected, "{kind:?}");
        }
    }

    #[test]
    fn codex_drops_the_subcommand_and_the_conversation_it_was_given() {
        let typed = resumed(
            AgentKind::Codex,
            &["codex", "-c", "model_reasoning_effort=high", "resume", "old-id", "--last", "keep going", "--search"],
            "bash",
        );
        assert_eq!(typed, format!("codex resume {SESSION} -c model_reasoning_effort=high --search"));
    }

    #[test]
    fn an_agent_running_on_a_runtime_is_started_through_the_same_script() {
        let typed = resumed(
            AgentKind::Claude,
            &["node", "--no-warnings", "/opt/claude/cli.js", "--model", "opus", "start here"],
            "bash",
        );
        assert_eq!(typed, format!("node --no-warnings /opt/claude/cli.js --resume {SESSION} --model opus"));
    }

    #[test]
    fn each_shell_gets_arguments_quoted_its_own_way() {
        let command = [r"C:\Program Files\Claude\claude.exe", "--append-system-prompt", "it's; $(calc) `x`"];
        assert_eq!(
            resumed(AgentKind::Claude, &command, "powershell.exe"),
            format!(
                r"& 'C:\Program Files\Claude\claude.exe' --resume {SESSION} --append-system-prompt 'it''s; $(calc) `x`'"
            )
        );
        assert_eq!(
            resumed(
                AgentKind::Claude,
                &["/opt/my apps/claude", "--append-system-prompt", "it's; $(calc)"],
                "/bin/bash"
            ),
            format!(r"'/opt/my apps/claude' --resume {SESSION} --append-system-prompt 'it'\''s; $(calc)'")
        );
        assert_eq!(
            resumed(AgentKind::Claude, &["claude", "--model=claude-opus-4.1", "--model", "opus-4.1"], "pwsh"),
            format!("claude --resume {SESSION} '--model=claude-opus-4.1' --model opus-4.1"),
            "PowerShell splits an unquoted option at its dot"
        );
    }

    #[test]
    fn a_command_no_shell_can_take_safely_falls_back_to_the_plain_program() {
        let plain = format!("claude --resume {SESSION}");
        for (command, shell) in [
            (vec!["claude", "--append-system-prompt", "two\nlines"], "bash"),
            (vec!["claude", "--append-system-prompt", "a\ttab"], "bash"),
            (vec!["claude", "--append-system-prompt", "say \"hi\""], "pwsh"),
            (vec!["claude", "--append-system-prompt", "curly \u{2019} quote"], "pwsh"),
            (vec!["claude", "--add-dir", r"C:\my dir\"], "pwsh"),
            (vec!["claude", "--append-system-prompt", "two words"], "cmd.exe"),
            (vec!["claude", "--append-system-prompt", "two words"], "nu"),
            (vec!["claude", "--append-system-prompt", ""], "bash"),
            (vec![], "bash"),
        ] {
            assert_eq!(resumed(AgentKind::Claude, &command, shell), plain, "{command:?} in {shell}");
        }
        let long = "x".repeat(MAX_TYPED_CHARS);
        assert_eq!(resumed(AgentKind::Claude, &["claude", "--name", &long], "bash"), plain);
        assert_eq!(super::command(&saved(AgentKind::Terminal, &[]), None, "bash"), None, "nothing to type");
    }

    #[test]
    fn shells_are_told_apart_by_their_program() {
        assert_eq!(shell_of(r"C:\Program Files\PowerShell\7\pwsh.exe"), Quoting::PowerShell);
        assert_eq!(shell_of("powershell"), Quoting::PowerShell);
        assert_eq!(shell_of(r"C:\Windows\System32\cmd.exe"), Quoting::Cmd);
        assert_eq!(shell_of("/usr/bin/zsh"), Quoting::Posix);
        assert_eq!(shell_of("/usr/bin/fish"), Quoting::Other);
    }
}
