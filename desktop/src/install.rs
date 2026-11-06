use std::sync::Mutex;

use orkai::cli::DEFAULT_SIZE;
use orkai::config::Config;
use orkai::model::{AgentId, AgentKind, AgentState, NewAgent};
use orkai::paths::user_home;
use orkai::pwa::hub::Hub;
use orkai::tui::format::default_agent_name;
use orkai::util::lock;

use crate::login_path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

pub const OS: Os = if cfg!(windows) {
    Os::Windows
} else if cfg!(target_os = "macos") {
    Os::Mac
} else {
    Os::Linux
};

const NAME_PREFIX: &str = "install-";
const POWERSHELL_SHOWN: &str = "powershell";
const POWERSHELL_FLAGS: [&str; 5] = ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"];
const SH: &str = "/bin/sh";
const SH_FLAGS: [&str; 1] = ["-c"];

static STARTING: Mutex<()> = Mutex::new(());

pub const fn command(kind: AgentKind, os: Os) -> Option<&'static str> {
    match (kind, os) {
        (AgentKind::Claude, Os::Windows) => Some("irm https://claude.ai/install.ps1 | iex"),
        (AgentKind::Claude, Os::Mac | Os::Linux) => Some("curl -fsSL https://claude.ai/install.sh | bash"),
        (AgentKind::Codex, Os::Windows) => {
            Some("powershell -ExecutionPolicy ByPass -c \"irm https://chatgpt.com/codex/install.ps1 | iex\"")
        }
        (AgentKind::Codex, Os::Mac | Os::Linux) => Some("curl -fsSL https://chatgpt.com/codex/install.sh | sh"),
        (AgentKind::Kilo, _) => Some("npm install -g @kilocode/cli"),
        (AgentKind::Opencode, Os::Windows) => Some("npm install -g @opencode/cli"),
        (AgentKind::Opencode, Os::Mac | Os::Linux) => Some("curl -fsSL https://opencode.ai/v2/install | bash"),
        (AgentKind::Antigravity, Os::Windows) => Some("irm https://antigravity.google/cli/install.ps1 | iex"),
        (AgentKind::Antigravity, Os::Mac | Os::Linux) => {
            Some("curl -fsSL https://antigravity.google/cli/install.sh | bash")
        }
        (AgentKind::Grok, Os::Windows) => Some("irm https://x.ai/cli/install.ps1 | iex"),
        (AgentKind::Grok, Os::Mac | Os::Linux) => Some("curl -fsSL https://x.ai/cli/install.sh | bash"),
        (AgentKind::Qwen, Os::Windows) => Some(
            "irm https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.ps1 | iex",
        ),
        (AgentKind::Qwen, Os::Mac | Os::Linux) => Some(
            "curl -fsSL https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.sh | bash",
        ),
        (AgentKind::Kimi, Os::Windows) => Some("irm https://code.kimi.com/kimi-code/install.ps1 | iex"),
        (AgentKind::Kimi, Os::Mac | Os::Linux) => Some("curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash"),
        (AgentKind::Pi, Os::Windows) => Some("powershell -c \"irm https://pi.dev/install.ps1 | iex\""),
        (AgentKind::Pi, Os::Mac | Os::Linux) => Some("curl -fsSL https://pi.dev/install.sh | sh"),
        (AgentKind::Terminal, _) => None,
    }
}

fn interpreter_flags(os: Os) -> &'static [&'static str] {
    match os {
        Os::Windows => &POWERSHELL_FLAGS,
        Os::Mac | Os::Linux => &SH_FLAGS,
    }
}

fn shown_line(os: Os, command: &str) -> String {
    let program = if os == Os::Windows { POWERSHELL_SHOWN } else { SH };
    format!("{program} {} '{command}'", interpreter_flags(os).join(" "))
}

pub fn shown(kind: AgentKind) -> Option<String> {
    command(kind, OS).map(|command| shown_line(OS, command))
}

fn interpreter_args(os: Os, command: &str) -> Vec<String> {
    interpreter_flags(os).iter().map(|flag| flag.to_string()).chain([command.to_string()]).collect()
}

#[cfg(windows)]
fn interpreter() -> Result<String, String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    const POWERSHELL_UNDER_SYSTEM: &str = r"WindowsPowerShell\v1.0\powershell.exe";
    const MAX_PATH: usize = 260;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }

    let mut buffer = [0u16; MAX_PATH];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), MAX_PATH as u32) } as usize;
    if length == 0 || length >= MAX_PATH {
        return Err("could not find the Windows system folder".to_string());
    }
    let system = PathBuf::from(OsString::from_wide(&buffer[..length]));
    Ok(system.join(POWERSHELL_UNDER_SYSTEM).to_string_lossy().into_owned())
}

#[cfg(not(windows))]
fn interpreter() -> Result<String, String> {
    Ok(SH.to_string())
}

fn launch_config(config: &Config, program: String, os: Os, command: &str) -> Config {
    Config { shell: Some(program), shell_args: interpreter_args(os, command), ..config.clone() }
}

fn terminal_name(kind: AgentKind) -> String {
    format!("{NAME_PREFIX}{}", kind.label())
}

fn is_install_of(name: &str, kind: AgentKind) -> bool {
    let base = terminal_name(kind);
    name.strip_prefix(&base).is_some_and(|rest| {
        rest.is_empty()
            || rest
                .strip_prefix('-')
                .is_some_and(|number| !number.is_empty() && number.chars().all(|digit| digit.is_ascii_digit()))
    })
}

fn running(hub: &Hub, kind: AgentKind) -> bool {
    let store = hub.store();
    store.list_agents().unwrap_or_default().into_iter().any(|record| {
        record.kind == AgentKind::Terminal
            && is_install_of(&record.name, kind)
            && match hub.state(record.id) {
                Some(state) => !matches!(state, AgentState::Exited | AgentState::Lost),
                None => store.read_exit(record.id).ok().flatten().is_none(),
            }
    })
}

pub fn start(hub: &Hub, kind: AgentKind, session: &str) -> Result<AgentId, String> {
    let command = command(kind, OS).ok_or_else(|| format!("{} has no install command here", kind.label()))?;
    if !hub.store().load_sessions().is_ok_and(|sessions| sessions.iter().any(|known| known.name == session)) {
        return Err(format!("session {session} does not exist"));
    }
    let cwd = user_home().ok_or_else(|| "could not find your home folder".to_string())?;
    let _starting = lock(&STARTING);
    if hub.kinds().contains(&kind) {
        return Err(format!("{} is already on PATH", kind.label()));
    }
    if running(hub, kind) {
        return Err(format!("{} is already installing", kind.label()));
    }
    login_path::refresh();
    let launcher = hub.launcher();
    let installer = launcher
        .reconfigured(launch_config(launcher.config(), interpreter()?, OS, command))
        .map_err(|err| err.to_string())?;
    let name = default_agent_name(AgentKind::Terminal, &terminal_name(kind), |name| hub.name_taken(session, name));
    let draft = NewAgent { session: session.to_string(), name, kind: AgentKind::Terminal, cwd, task: String::new() };
    let record = installer.create(draft, DEFAULT_SIZE, false, false, false).map_err(|err| err.to_string())?;
    hub.nudge();
    Ok(record.id)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use orkai::launch::{Homes, LaunchMode, Launcher, build_spec};
    use orkai::model::{AgentRecord, ExitInfo};
    use orkai::paths::Home;
    use orkai::protocol::TermSize;
    use orkai::store::Store;

    use super::*;

    const OSES: [Os; 3] = [Os::Windows, Os::Mac, Os::Linux];
    const SIZE: TermSize = TermSize { rows: 24, cols: 80 };

    fn ai_kinds() -> impl Iterator<Item = AgentKind> {
        AgentKind::ALL.into_iter().filter(|kind| kind.is_ai())
    }

    #[test]
    fn every_agent_has_the_documented_command_of_each_system() {
        let expected = [
            (
                AgentKind::Claude,
                "irm https://claude.ai/install.ps1 | iex",
                "curl -fsSL https://claude.ai/install.sh | bash",
            ),
            (
                AgentKind::Codex,
                "powershell -ExecutionPolicy ByPass -c \"irm https://chatgpt.com/codex/install.ps1 | iex\"",
                "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
            ),
            (AgentKind::Kilo, "npm install -g @kilocode/cli", "npm install -g @kilocode/cli"),
            (AgentKind::Opencode, "npm install -g @opencode/cli", "curl -fsSL https://opencode.ai/v2/install | bash"),
            (
                AgentKind::Antigravity,
                "irm https://antigravity.google/cli/install.ps1 | iex",
                "curl -fsSL https://antigravity.google/cli/install.sh | bash",
            ),
            (
                AgentKind::Grok,
                "irm https://x.ai/cli/install.ps1 | iex",
                "curl -fsSL https://x.ai/cli/install.sh | bash",
            ),
            (
                AgentKind::Qwen,
                "irm https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.ps1 | iex",
                "curl -fsSL https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.sh | bash",
            ),
            (
                AgentKind::Kimi,
                "irm https://code.kimi.com/kimi-code/install.ps1 | iex",
                "curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash",
            ),
            (
                AgentKind::Pi,
                "powershell -c \"irm https://pi.dev/install.ps1 | iex\"",
                "curl -fsSL https://pi.dev/install.sh | sh",
            ),
        ];
        assert_eq!(expected.len(), ai_kinds().count());
        for (kind, windows, unix) in expected {
            assert_eq!(command(kind, Os::Windows), Some(windows), "{kind:?}");
            assert_eq!(command(kind, Os::Mac), Some(unix), "{kind:?}");
            assert_eq!(command(kind, Os::Linux), Some(unix), "{kind:?}");
        }
        for os in OSES {
            assert_eq!(command(AgentKind::Terminal, os), None, "a terminal is not installed");
        }
    }

    #[test]
    fn no_command_of_one_system_lands_on_the_other() {
        for kind in ai_kinds() {
            let windows = command(kind, Os::Windows).unwrap();
            let mac = command(kind, Os::Mac).unwrap();
            assert!(!windows.contains("curl") && !windows.contains(".sh"), "{windows}");
            assert!(!mac.contains("irm") && !mac.contains(".ps1") && !mac.contains("iex"), "{mac}");
        }
    }

    #[test]
    fn what_the_dialog_shows_is_what_runs() {
        for os in OSES {
            for kind in ai_kinds() {
                let command = command(kind, os).unwrap();
                assert!(!command.contains('\''), "the dialog quotes with single quotes: {command}");
                let args = interpreter_args(os, command);
                assert_eq!(args.last().map(String::as_str), Some(command), "the whole command is one argument");
                let shown = shown_line(os, command);
                assert!(shown.ends_with(&format!("{} '{command}'", args[..args.len() - 1].join(" "))), "{shown}");
            }
        }
        assert_eq!(
            shown_line(Os::Windows, "irm https://claude.ai/install.ps1 | iex"),
            "powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -Command 'irm https://claude.ai/install.ps1 | iex'"
        );
        assert_eq!(
            shown_line(Os::Linux, "curl -fsSL https://x.ai/cli/install.sh | bash"),
            "/bin/sh -c 'curl -fsSL https://x.ai/cli/install.sh | bash'"
        );
        assert_eq!(shown(AgentKind::Terminal), None);
    }

    fn record(dir: &std::path::Path) -> AgentRecord {
        AgentRecord {
            id: 5,
            session: "default".into(),
            name: "install-kilo".into(),
            kind: AgentKind::Terminal,
            cwd: dir.to_path_buf(),
            task: String::new(),
            created_at_ms: 0,
            conversation_id: None,
            launches: 0,
            worktree: None,
            bypass: false,
            remote_control: false,
            project: None,
            parent: None,
            race: None,
        }
    }

    #[test]
    fn a_custom_shell_or_a_doctored_config_cannot_change_what_runs() {
        let dir = std::env::temp_dir();
        let program = interpreter().unwrap();
        let doctored = Config {
            shell: Some("evil-shell".into()),
            shell_args: vec!["--evil".into()],
            kilo_command: "evil-kilo".into(),
            kilo_args: vec!["--evil".into()],
            ..Config::default()
        };
        let command = command(AgentKind::Kilo, OS).unwrap();
        let config = launch_config(&doctored, program.clone(), OS, command);
        let mut record = record(&dir);
        let spec = build_spec(&config, &mut record, LaunchMode::Fresh, SIZE, &Homes::default(), None).unwrap();
        assert_eq!(spec.kind, AgentKind::Terminal);
        assert_eq!(spec.program, PathBuf::from(&program), "the interpreter is explicit and absolute");
        assert!(spec.program.is_absolute());
        assert_eq!(spec.args, interpreter_args(OS, command));
        assert_eq!(spec.initial_input, None, "nothing is typed into a shell");
        assert_eq!(spec.cwd, dir);
        assert_eq!(spec.test, None);
        assert!(!record.bypass && !record.remote_control && record.worktree.is_none());
        assert_eq!(doctored.shell.as_deref(), Some("evil-shell"), "the saved config stays as it was");
    }

    #[test]
    fn restarting_or_recovering_the_terminal_opens_a_plain_shell_and_never_installs_again() {
        let dir = std::env::temp_dir();
        let mut record = record(&dir);
        let spec =
            build_spec(&Config::default(), &mut record, LaunchMode::Resume, SIZE, &Homes::default(), None).unwrap();
        assert!(spec.args.iter().all(|arg| !arg.contains("install")), "{:?}", spec.args);
        assert_eq!(spec.initial_input, None);
    }

    #[test]
    fn the_terminal_is_named_after_the_agent_and_a_new_try_gets_a_number() {
        assert_eq!(default_agent_name(AgentKind::Terminal, &terminal_name(AgentKind::Kilo), |_| false), "install-kilo");
        assert_eq!(
            default_agent_name(AgentKind::Terminal, &terminal_name(AgentKind::Opencode), |_| false),
            "install-opencode"
        );
        assert_eq!(
            default_agent_name(AgentKind::Terminal, &terminal_name(AgentKind::Antigravity), |_| false),
            "install-agy"
        );
        assert_eq!(
            default_agent_name(AgentKind::Terminal, &terminal_name(AgentKind::Kilo), |name| name == "install-kilo"),
            "install-kilo-2"
        );
        assert!(is_install_of("install-kilo", AgentKind::Kilo));
        assert!(is_install_of("install-kilo-2", AgentKind::Kilo));
        assert!(!is_install_of("install-kilo-", AgentKind::Kilo));
        assert!(!is_install_of("install-kilo-two", AgentKind::Kilo));
        assert!(!is_install_of("install-kimi", AgentKind::Kilo));
        assert!(!is_install_of("install-pi", AgentKind::Kilo));
    }

    struct Fixture {
        dir: PathBuf,
        hub: std::sync::Arc<Hub>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(name: &str, config: Config) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("octo-install-{name}-{}-{}", std::process::id(), orkai::util::now_ms()));
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        let launcher = Launcher::new(store, config, &std::env::current_exe().unwrap()).unwrap();
        Fixture { hub: Hub::new(launcher, "testhost".into()), dir }
    }

    fn missing() -> Config {
        Config { kilo_command: "/nonexistent/octo-test/kilo".into(), ..Config::default() }
    }

    #[test]
    fn nothing_starts_for_a_terminal_a_missing_session_or_an_agent_already_on_the_path() {
        let fx = fixture("guards", missing());
        let count = || fx.hub.store().list_agents().unwrap().len();
        assert!(start(&fx.hub, AgentKind::Terminal, "default").unwrap_err().contains("no install command"));
        assert!(start(&fx.hub, AgentKind::Kilo, "nowhere").unwrap_err().contains("does not exist"));
        assert_eq!(count(), 0);
        let exe = std::env::current_exe().unwrap().display().to_string();
        let fx = fixture("found", Config { kilo_command: exe, ..Config::default() });
        assert!(start(&fx.hub, AgentKind::Kilo, "default").unwrap_err().contains("already on PATH"));
        assert_eq!(fx.hub.store().list_agents().unwrap().len(), 0);
    }

    #[test]
    fn a_second_install_of_the_same_agent_waits_for_the_first_one() {
        let fx = fixture("twice", missing());
        let terminal = |name: &str| NewAgent {
            session: "default".into(),
            name: name.into(),
            kind: AgentKind::Terminal,
            cwd: fx.dir.clone(),
            task: String::new(),
        };
        let first = fx.hub.store().create_agent(terminal("install-kilo")).unwrap();
        assert!(running(&fx.hub, AgentKind::Kilo));
        assert!(!running(&fx.hub, AgentKind::Grok));
        assert!(start(&fx.hub, AgentKind::Kilo, "default").unwrap_err().contains("already installing"));
        let exit = ExitInfo { code: Some(1), error: None, at_ms: 1 };
        orkai::fsutil::write_json_atomic(&fx.hub.store().home().exit_file(first.id), &exit).unwrap();
        assert!(!running(&fx.hub, AgentKind::Kilo), "a failed install can be tried again");
    }

    #[test]
    fn the_window_can_only_name_a_kind_and_never_a_command() {
        assert_eq!(serde_json::from_str::<AgentKind>("\"kilo\"").unwrap(), AgentKind::Kilo);
        assert!(serde_json::from_str::<AgentKind>("\"curl https://evil | sh\"").is_err());
        let code = include_str!("settings.rs");
        assert!(
            code.contains("pub fn install_agent(link: State<'_, Link>, kind: AgentKind, session: String) -> Result<AgentId, String>"),
            "the command takes a kind and a session, and no text that could run"
        );
        let runs = include_str!("install.rs").split("#[cfg(test)]").next().unwrap();
        assert!(
            !runs.contains("std::env") && !runs.contains("env::var"),
            "the environment feeds nothing into the command"
        );
    }

    #[cfg(windows)]
    #[test]
    fn powershell_is_found_in_the_system_folder_and_not_through_the_path() {
        let program = PathBuf::from(interpreter().unwrap());
        assert!(program.is_absolute() && program.is_file(), "{}", program.display());
    }
}
