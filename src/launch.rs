use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::fsutil;
use crate::hooks;
use crate::model::{AgentId, AgentKind, AgentRecord, LaunchSpec, NewAgent, RuntimeInfo};
use crate::paths::{HOME_ENV, Home, normalize_for_compare, user_home};
use crate::protocol::TermSize;
use crate::store::Store;
use crate::util::{civil_date_from_ms, fnv1a64, is_uuid, new_uuid_v4};
use crate::worktree;

pub const HOLDER_COMMAND: &str = "__holder";
pub const HOLDER_PREFIX: &str = "orkai-holder-";
const RUNTIME_POLL: Duration = Duration::from_millis(25);
const DAY_MS: u64 = 86_400_000;
const CODEX_CLOCK_SKEW_MS: u64 = 60_000;
const ENV_REMOVE: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    crate::update::UPDATED_ENV,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchMode {
    Fresh,
    Resume,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

pub fn resolve_program(name: &str) -> io::Result<PathBuf> {
    let path = Path::new(name);
    if path.components().count() > 1 || path.is_absolute() {
        return if path.exists() {
            Ok(path.to_path_buf())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, format!("{name} not found")))
        };
    }
    which::which(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("'{name}' was not found in PATH; install it or set its command in config.json"),
        )
    })
}

pub fn default_shell(config: &Config) -> (String, Vec<String>) {
    if let Some(shell) = config.shell.as_ref().filter(|s| !s.trim().is_empty()) {
        return (shell.clone(), config.shell_args.clone());
    }
    if cfg!(windows) {
        let shell = if which::which("pwsh").is_ok() { "pwsh" } else { "powershell" };
        (shell.to_string(), vec!["-NoLogo".to_string()])
    } else {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
            if Path::new("/bin/bash").exists() { "/bin/bash".to_string() } else { "/bin/sh".to_string() }
        });
        (shell, Vec::new())
    }
}

const CMD_UNSAFE: &[char] = &['&', '|', '<', '>', '^', '%', '"', '(', ')', '!', '\r', '\n'];

pub fn wrap_for_platform(program: PathBuf, args: Vec<String>) -> io::Result<(PathBuf, Vec<String>)> {
    let is_batch = cfg!(windows)
        && program
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .is_some_and(|e| e == "cmd" || e == "bat");
    if !is_batch {
        return Ok((program, args));
    }
    let program_text = program.to_string_lossy().into_owned();
    if program_text.contains(CMD_UNSAFE) || args.iter().any(|a| a.contains(CMD_UNSAFE)) {
        return Err(invalid(format!(
            "refusing to run {program_text} through cmd.exe with special characters in its arguments"
        )));
    }
    let comspec =
        std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
    let mut wrapped = vec!["/d".to_string(), "/c".to_string(), program_text];
    wrapped.extend(args);
    Ok((comspec, wrapped))
}

pub fn claude_home() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| user_home().map(|h| h.join(".claude")))
}

pub fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| user_home().map(|h| h.join(".codex")))
}

pub fn claude_transcript_exists(claude_home: &Path, session_id: &str) -> bool {
    let file = format!("{session_id}.jsonl");
    fs::read_dir(claude_home.join("projects"))
        .map(|entries| entries.flatten().any(|e| e.path().join(&file).is_file()))
        .unwrap_or(false)
}

fn sorted_dir_names(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(path)
        .map(|entries| entries.flatten().filter_map(|e| e.file_name().to_str().map(str::to_string)).collect())
        .unwrap_or_default();
    names.sort();
    names
}

fn codex_meta(line: &str) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let source = value.get("payload").filter(|p| p.get("id").is_some()).unwrap_or(&value);
    if source.get("source").and_then(|s| s.get("subagent")).is_some() {
        return None;
    }
    let id = source.get("id")?.as_str()?.to_string();
    let cwd = source.get("cwd")?.as_str()?.to_string();
    Some((id, cwd))
}

pub fn is_safe_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn recent_codex_rollouts(codex_home: &Path, since_ms: u64) -> Vec<PathBuf> {
    let root = codex_home.join("sessions");
    let (y, m, d) = civil_date_from_ms(since_ms.saturating_sub(DAY_MS));
    let floor = format!("{y:04}/{m:02}/{d:02}");
    let threshold = since_ms.saturating_sub(CODEX_CLOCK_SKEW_MS);
    let mut files: Vec<PathBuf> = Vec::new();
    for year in sorted_dir_names(&root) {
        for month in sorted_dir_names(&root.join(&year)) {
            for day in sorted_dir_names(&root.join(&year).join(&month)) {
                if format!("{year}/{month}/{day}") < floor {
                    continue;
                }
                let dir = root.join(&year).join(&month).join(&day);
                for name in sorted_dir_names(&dir) {
                    if !(name.starts_with("rollout-") && name.ends_with(".jsonl")) {
                        continue;
                    }
                    let path = dir.join(&name);
                    let modified = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
                        .unwrap_or(0);
                    if modified >= threshold {
                        files.push(path);
                    }
                }
            }
        }
    }
    files
}

pub fn find_codex_session(codex_home: &Path, cwd: &Path, since_ms: u64, exclude: &[String]) -> Option<String> {
    let target = normalize_for_compare(cwd);
    for path in recent_codex_rollouts(codex_home, since_ms) {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let Some((id, session_cwd)) = text.lines().next().and_then(codex_meta) else { continue };
        if normalize_for_compare(Path::new(&session_cwd)) == target && !exclude.contains(&id) && is_safe_session_id(&id)
        {
            return Some(id);
        }
    }
    None
}

pub struct HookSetup {
    pub exe: PathBuf,
    pub claude_settings: PathBuf,
    pub codex_home: Option<PathBuf>,
}

fn names_option(args: &[String], option: &str) -> bool {
    args.iter().any(|arg| arg == option || arg.starts_with(&format!("{option}=")))
}

fn hook_args(config: &Config, record: &AgentRecord, program: &Path, setup: &HookSetup) -> Vec<String> {
    match record.kind {
        AgentKind::Claude => {
            if names_option(&config.claude_args, "--settings")
                || !hooks::git_bash_available()
                || !hooks::claude_supports_hooks(program)
            {
                return Vec::new();
            }
            let settings = hooks::claude_settings(&hooks::claude_hook_command(&setup.exe, record.id));
            match fsutil::write_json_atomic(&setup.claude_settings, &settings) {
                Ok(()) => vec!["--settings".to_string(), setup.claude_settings.to_string_lossy().into_owned()],
                Err(_) => Vec::new(),
            }
        }
        AgentKind::Codex => {
            let configured = config.codex_args.iter().any(|arg| arg.trim_start().starts_with("notify"))
                || setup.codex_home.as_deref().is_some_and(hooks::codex_has_own_notify);
            if configured {
                return Vec::new();
            }
            vec!["-c".to_string(), hooks::codex_notify_override(&setup.exe, record.id)]
        }
        AgentKind::Terminal => Vec::new(),
    }
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub fn build_spec(
    config: &Config,
    record: &mut AgentRecord,
    mode: LaunchMode,
    size: TermSize,
    claude_dir: Option<&Path>,
    hooks: Option<&HookSetup>,
) -> io::Result<LaunchSpec> {
    let (program_name, args, initial_input) = match record.kind {
        AgentKind::Claude => {
            let session_id = match record.conversation_id.as_deref() {
                Some(id) if is_uuid(id) => id.to_string(),
                _ => {
                    let id = new_uuid_v4()?;
                    record.conversation_id = Some(id.clone());
                    id
                }
            };
            let resume =
                mode == LaunchMode::Resume && claude_dir.is_some_and(|dir| claude_transcript_exists(dir, &session_id));
            let mut args = config.claude_args.clone();
            args.push("--remote-control".to_string());
            if resume {
                args.extend(["--resume".to_string(), session_id]);
            } else {
                args.extend(["--session-id".to_string(), session_id]);
            }
            let input = if resume { None } else { non_empty(&record.task) };
            (config.claude_command.clone(), args, input)
        }
        AgentKind::Codex => {
            let resume_id =
                record.conversation_id.clone().filter(|id| mode == LaunchMode::Resume && is_safe_session_id(id));
            match resume_id {
                Some(id) => {
                    let mut args = vec!["resume".to_string(), id];
                    args.extend(config.codex_args.clone());
                    (config.codex_command.clone(), args, None)
                }
                None => (config.codex_command.clone(), config.codex_args.clone(), non_empty(&record.task)),
            }
        }
        AgentKind::Terminal => {
            let (shell, args) = default_shell(config);
            (shell, args, non_empty(&record.task))
        }
    };
    if !record.cwd.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("directory {} does not exist", record.cwd.display()),
        ));
    }
    let resolved = resolve_program(&program_name)?;
    let extra = hooks.map(|setup| hook_args(config, record, &resolved, setup)).unwrap_or_default();
    let with_hooks = match record.kind {
        AgentKind::Codex => extra.iter().chain(&args).cloned().collect(),
        _ => args.iter().chain(&extra).cloned().collect(),
    };
    let (program, args) = match wrap_for_platform(resolved.clone(), with_hooks) {
        Ok(wrapped) => wrapped,
        Err(_) if !extra.is_empty() => wrap_for_platform(resolved, args)?,
        Err(err) => return Err(err),
    };
    let mut env = vec![
        ("ORKAI_AGENT_ID".to_string(), record.id.to_string()),
        ("ORKAI_SESSION".to_string(), record.session.clone()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
    ];
    if cfg!(unix) {
        env.push(("TERM".to_string(), "xterm-256color".to_string()));
    }
    Ok(LaunchSpec {
        kind: record.kind,
        program,
        args,
        cwd: record.cwd.clone(),
        env,
        env_remove: ENV_REMOVE.iter().map(|s| s.to_string()).collect(),
        initial_input,
        rows: size.rows,
        cols: size.cols,
    })
}

static STAGED: OnceLock<Mutex<Option<(PathBuf, PathBuf)>>> = OnceLock::new();

pub fn holder_executable(home: &Home, source: &Path) -> io::Result<PathBuf> {
    let cache = STAGED.get_or_init(|| Mutex::new(None));
    let mut guard = crate::util::lock(cache);
    if let Some((cached_source, target)) = guard.as_ref()
        && cached_source == source
        && target.exists()
    {
        return Ok(target.clone());
    }
    let data = fs::read(source)?;
    let name =
        format!("{HOLDER_PREFIX}{}-{:016x}{}", env!("CARGO_PKG_VERSION"), fnv1a64(&data), std::env::consts::EXE_SUFFIX);
    let target = home.bin_dir().join(name);
    if !target.exists() {
        let tmp = target.with_file_name(format!(".staging-{}-{}", std::process::id(), crate::util::now_ms()));
        fs::write(&tmp, &data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o700))?;
        }
        if let Err(err) = fs::rename(&tmp, &target) {
            let _ = fs::remove_file(&tmp);
            if !target.exists() {
                return Err(err);
            }
        }
    }
    *guard = Some((source.to_path_buf(), target.clone()));
    Ok(target)
}

fn holder_binaries_in_use(home: &Home) -> HashSet<String> {
    let Ok(entries) = fs::read_dir(home.agents_dir()) else { return HashSet::new() };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().and_then(|name| name.parse::<AgentId>().ok()))
        .filter_map(|id| fsutil::read_json_opt::<RuntimeInfo>(&home.runtime_file(id)).ok().flatten())
        .filter_map(|runtime| runtime.exe)
        .map(|exe| normalize_for_compare(&exe))
        .collect()
}

pub fn prune_holder_binaries(home: &Home, keep: &Path) {
    let Ok(entries) = fs::read_dir(home.bin_dir()) else { return };
    let in_use = holder_binaries_in_use(home);
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if (name.starts_with(HOLDER_PREFIX) || name.starts_with(".staging-"))
            && path != keep
            && !in_use.contains(&normalize_for_compare(&path))
        {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn spawn_holder(home: &Home, id: AgentId, exe: &Path) -> io::Result<()> {
    let mut cmd = Command::new(exe);
    cmd.arg(HOLDER_COMMAND)
        .arg(id.to_string())
        .env(HOME_ENV, home.root())
        .current_dir(home.root())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = spawn_detached(&mut cmd)?;
    std::thread::Builder::new().name("orkai-holder-reaper".into()).spawn(move || {
        let _ = child.wait();
    })?;
    Ok(())
}

#[cfg(windows)]
fn disable_std_handle_inheritance() {
    use std::ffi::c_void;
    const STD_HANDLES: [u32; 3] = [-10i32 as u32, -11i32 as u32, -12i32 as u32];
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(std_handle: u32) -> *mut c_void;
        fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for id in STD_HANDLES {
            unsafe {
                let handle = GetStdHandle(id);
                if !handle.is_null() && handle as isize != -1 {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    });
}

#[cfg(windows)]
pub fn spawn_detached(cmd: &mut Command) -> io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    disable_std_handle_inheritance();
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
    match cmd.spawn() {
        Ok(child) => Ok(child),
        Err(_) => {
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
            cmd.spawn()
        }
    }
}

#[cfg(unix)]
pub fn spawn_detached(cmd: &mut Command) -> io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn()
}

pub fn wait_for_runtime(store: &Store, id: AgentId, timeout: Duration) -> io::Result<RuntimeInfo> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(runtime) = store.read_runtime(id).ok().flatten() {
            return Ok(runtime);
        }
        if let Some(exit) = store.read_exit(id).ok().flatten() {
            return Err(io::Error::other(exit.describe()));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "agent did not start in time"));
        }
        std::thread::sleep(RUNTIME_POLL);
    }
}

pub fn process_is_holder(pid: u32) -> bool {
    let output = if cfg!(windows) {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
    } else {
        Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
    };
    output
        .map(|o| {
            let text = String::from_utf8_lossy(&o.stdout).to_ascii_lowercase();
            text.contains(HOLDER_PREFIX)
        })
        .unwrap_or(false)
}

pub fn kill_process_tree(pid: u32) {
    if cfg!(windows) {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    } else {
        #[cfg(unix)]
        if let Ok(pid) = libc::pid_t::try_from(pid) {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

#[derive(Clone)]
pub struct Launcher {
    store: Store,
    config: Config,
    source: PathBuf,
    exe: PathBuf,
}

impl Launcher {
    pub fn new(store: Store, config: Config, source_exe: &Path) -> io::Result<Self> {
        let exe = holder_executable(store.home(), source_exe)?;
        Ok(Self { store, config, source: source_exe.to_path_buf(), exe })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn holder_exe(&self) -> &Path {
        &self.exe
    }

    pub fn create(&self, draft: NewAgent, size: TermSize, isolate: bool) -> io::Result<AgentRecord> {
        let mut record = self.store.create_agent(draft)?;
        let started =
            self.isolate(&mut record, isolate).and_then(|()| self.start(&mut record, LaunchMode::Fresh, size));
        if let Err(err) = started {
            if let Some(worktree) = record.worktree.as_ref() {
                let _ = worktree::discard(self.store.home(), worktree);
            }
            let _ = self.store.remove_agent(record.id);
            return Err(err);
        }
        Ok(record)
    }

    fn isolate(&self, record: &mut AgentRecord, isolate: bool) -> io::Result<()> {
        if !isolate {
            return Ok(());
        }
        let (worktree, cwd) = worktree::create(self.store.home(), record.id, &record.name, &record.cwd)?;
        record.cwd = cwd;
        record.worktree = Some(worktree);
        self.store.save_agent(record)
    }

    pub fn start(&self, record: &mut AgentRecord, mode: LaunchMode, size: TermSize) -> io::Result<()> {
        if mode == LaunchMode::Resume {
            self.stop_stale_holder(record.id);
            if record.kind == AgentKind::Codex && record.conversation_id.is_none() {
                record.conversation_id = self.discover_codex_session(record)?;
            }
        }
        let exe = holder_executable(self.store.home(), &self.source).unwrap_or_else(|_| self.exe.clone());
        let setup = HookSetup {
            exe: exe.clone(),
            claude_settings: self.store.home().claude_settings_file(record.id),
            codex_home: codex_home(),
        };
        let spec = build_spec(&self.config, record, mode, size, claude_home().as_deref(), Some(&setup))?;
        record.launches = record.launches.saturating_add(1);
        self.store.save_agent(record)?;
        self.store.clear_runtime_markers(record.id)?;
        self.store.write_launch(record.id, &spec)?;
        spawn_holder(self.store.home(), record.id, &exe)
    }

    fn discover_codex_session(&self, record: &AgentRecord) -> io::Result<Option<String>> {
        let Some(home) = codex_home() else { return Ok(None) };
        let claimed: Vec<String> = self
            .store
            .list_agents()?
            .into_iter()
            .filter(|r| r.id != record.id && r.kind == AgentKind::Codex)
            .filter_map(|r| r.conversation_id)
            .collect();
        Ok(find_codex_session(&home, &record.cwd, record.created_at_ms, &claimed))
    }

    fn stop_stale_holder(&self, id: AgentId) {
        if let Ok(Some(runtime)) = self.store.read_runtime(id)
            && runtime.holder_pid != std::process::id()
            && process_is_holder(runtime.holder_pid)
        {
            kill_process_tree(runtime.holder_pid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orkai-launch-{tag}-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(kind: AgentKind, cwd: PathBuf, task: &str) -> AgentRecord {
        AgentRecord {
            id: 5,
            session: "default".into(),
            name: "agent".into(),
            kind,
            cwd,
            task: task.into(),
            created_at_ms: now_ms(),
            conversation_id: None,
            launches: 0,
            worktree: None,
        }
    }

    fn config_with(program: &Path) -> Config {
        Config {
            shell: Some(program.to_string_lossy().into_owned()),
            claude_command: program.to_string_lossy().into_owned(),
            codex_command: program.to_string_lossy().into_owned(),
            ..Config::default()
        }
    }

    fn fake_program(dir: &Path) -> PathBuf {
        let path = dir.join(if cfg!(windows) { "fake.exe" } else { "fake" });
        fs::write(&path, b"").unwrap();
        path
    }

    #[test]
    fn fresh_claude_gets_new_session_id_and_task_as_input() {
        let dir = temp_dir("claude-fresh");
        let program = fake_program(&dir);
        let mut r = record(AgentKind::Claude, dir.clone(), "  fix the build ");
        let spec =
            build_spec(&config_with(&program), &mut r, LaunchMode::Fresh, TermSize::new(30, 100), None, None).unwrap();
        let id = r.conversation_id.clone().unwrap();
        assert!(is_uuid(&id));
        assert_eq!(spec.args, vec!["--remote-control".to_string(), "--session-id".to_string(), id]);
        assert_eq!(spec.initial_input.as_deref(), Some("fix the build"));
        assert_eq!((spec.rows, spec.cols), (30, 100));
        assert!(spec.env.iter().any(|(k, v)| k == "ORKAI_AGENT_ID" && v == "5"));
        assert!(spec.env_remove.contains(&"CLAUDECODE".to_string()));
        assert!(spec.env_remove.contains(&"CLAUDE_CODE_CHILD_SESSION".to_string()));
        assert!(spec.env_remove.contains(&"CLAUDE_CODE_SESSION_ID".to_string()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claude_resume_uses_existing_transcript() {
        let dir = temp_dir("claude-resume");
        let program = fake_program(&dir);
        let claude_dir = dir.join("claude-home");
        let id = "123e4567-e89b-42d3-a456-426614174000";
        fs::create_dir_all(claude_dir.join("projects").join("C--work")).unwrap();
        fs::write(claude_dir.join("projects").join("C--work").join(format!("{id}.jsonl")), b"{}").unwrap();
        let mut r = record(AgentKind::Claude, dir.clone(), "task");
        r.conversation_id = Some(id.into());
        let spec = build_spec(
            &config_with(&program),
            &mut r,
            LaunchMode::Resume,
            TermSize::new(24, 80),
            Some(&claude_dir),
            None,
        )
        .unwrap();
        assert_eq!(spec.args, vec!["--remote-control".to_string(), "--resume".to_string(), id.to_string()]);
        assert_eq!(spec.initial_input, None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claude_resume_without_transcript_restarts_with_same_id() {
        let dir = temp_dir("claude-no-transcript");
        let program = fake_program(&dir);
        let id = "123e4567-e89b-42d3-a456-426614174000";
        let mut r = record(AgentKind::Claude, dir.clone(), "task");
        r.conversation_id = Some(id.into());
        let spec =
            build_spec(&config_with(&program), &mut r, LaunchMode::Resume, TermSize::new(24, 80), Some(&dir), None)
                .unwrap();
        assert_eq!(spec.args, vec!["--remote-control".to_string(), "--session-id".to_string(), id.to_string()]);
        assert_eq!(spec.initial_input.as_deref(), Some("task"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claude_keeps_configured_args_and_always_enables_remote_control() {
        let dir = temp_dir("claude-remote-control");
        let program = fake_program(&dir);
        let config = Config { claude_args: vec!["--model".into(), "opus".into()], ..config_with(&program) };
        let id = "123e4567-e89b-42d3-a456-426614174000";
        let mut r = record(AgentKind::Claude, dir.clone(), "task");
        r.conversation_id = Some(id.into());
        let spec = build_spec(&config, &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, None).unwrap();
        assert_eq!(spec.args, vec!["--model", "opus", "--remote-control", "--session-id", id]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_resume_uses_known_conversation() {
        let dir = temp_dir("codex");
        let program = fake_program(&dir);
        let mut r = record(AgentKind::Codex, dir.clone(), "task");
        r.conversation_id = Some("abc-123".into());
        let spec =
            build_spec(&config_with(&program), &mut r, LaunchMode::Resume, TermSize::new(24, 80), None, None).unwrap();
        assert_eq!(spec.args, vec!["resume".to_string(), "abc-123".to_string()]);
        assert_eq!(spec.initial_input, None);
        let fresh =
            build_spec(&config_with(&program), &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, None).unwrap();
        assert!(fresh.args.is_empty());
        assert_eq!(fresh.initial_input.as_deref(), Some("task"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn terminal_reruns_its_command() {
        let dir = temp_dir("terminal");
        let program = fake_program(&dir);
        let mut r = record(AgentKind::Terminal, dir.clone(), "npm run dev");
        let spec =
            build_spec(&config_with(&program), &mut r, LaunchMode::Resume, TermSize::new(24, 80), None, None).unwrap();
        assert_eq!(spec.program, program);
        assert_eq!(spec.initial_input.as_deref(), Some("npm run dev"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_directory_is_rejected() {
        let dir = temp_dir("missing");
        let program = fake_program(&dir);
        let mut r = record(AgentKind::Terminal, dir.join("does-not-exist"), "");
        let err = build_spec(&config_with(&program), &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, None)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_program_reports_not_found() {
        let err = resolve_program("orkai-definitely-missing-program").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(windows)]
    #[test]
    fn batch_files_are_wrapped_and_validated() {
        let (program, args) =
            wrap_for_platform(PathBuf::from(r"C:\npm\claude.cmd"), vec!["--session-id".into(), "x".into()]).unwrap();
        assert!(program.to_string_lossy().to_ascii_lowercase().ends_with("cmd.exe"));
        assert_eq!(args, vec!["/d", "/c", r"C:\npm\claude.cmd", "--session-id", "x"]);
        assert!(wrap_for_platform(PathBuf::from(r"C:\npm\claude.cmd"), vec!["a&calc".into()]).is_err());
        let (exe, exe_args) = wrap_for_platform(PathBuf::from(r"C:\bin\claude.exe"), vec!["a&b".into()]).unwrap();
        assert_eq!(exe, PathBuf::from(r"C:\bin\claude.exe"));
        assert_eq!(exe_args, vec!["a&b"]);
    }

    #[test]
    fn finds_codex_session_by_cwd_and_time() {
        let dir = temp_dir("codex-home");
        let since = now_ms();
        let (y, m, d) = civil_date_from_ms(since);
        let day = dir.join("sessions").join(format!("{y:04}")).join(format!("{m:02}")).join(format!("{d:02}"));
        fs::create_dir_all(&day).unwrap();
        let project = dir.join("project");
        let other = dir.join("other");
        let meta = |id: &str, cwd: &Path| {
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":{}}}}}\n",
                serde_json::to_string(&cwd.to_string_lossy()).unwrap()
            )
        };
        fs::write(day.join("rollout-a-1.jsonl"), meta("other-session", &other)).unwrap();
        fs::write(day.join("rollout-b-2.jsonl"), meta("claimed-session", &project)).unwrap();
        fs::write(day.join("rollout-c-3.jsonl"), meta("wanted-session", &project)).unwrap();
        let found = find_codex_session(&dir, &project, since, &["claimed-session".to_string()]);
        assert_eq!(found.as_deref(), Some("wanted-session"));
        assert_eq!(find_codex_session(&dir, &dir.join("nowhere"), since, &[]), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_meta_supports_flat_format() {
        assert_eq!(
            codex_meta(r#"{"id":"abc","cwd":"/x","timestamp":"t"}"#),
            Some(("abc".to_string(), "/x".to_string()))
        );
        assert_eq!(codex_meta("not json"), None);
    }

    #[test]
    fn codex_subagent_threads_are_never_taken_for_the_main_session() {
        let main = r#"{"type":"session_meta","payload":{"id":"main-1","cwd":"/x","source":"cli"}}"#;
        assert_eq!(codex_meta(main), Some(("main-1".to_string(), "/x".to_string())));
        let spawned = r#"{"type":"session_meta","payload":{"id":"child-1","cwd":"/x","source":{"subagent":{"thread_spawn":{"parent_thread_id":"main-1"}}}}}"#;
        assert_eq!(codex_meta(spawned), None);
    }

    #[test]
    fn session_ids_must_be_safe() {
        assert!(is_safe_session_id("0199a-abc_DEF"));
        assert!(!is_safe_session_id("a&b"));
        assert!(!is_safe_session_id(""));
    }

    #[test]
    fn holder_binary_is_staged_once_and_pruned() {
        let dir = temp_dir("stage");
        let home = Home::at(dir.join("home"));
        home.ensure().unwrap();
        let source = dir.join("source-bin");
        fs::write(&source, b"binary-content").unwrap();
        let staged = holder_executable(&home, &source).unwrap();
        assert!(staged.exists());
        assert!(staged.file_name().unwrap().to_string_lossy().starts_with(HOLDER_PREFIX));
        assert_eq!(holder_executable(&home, &source).unwrap(), staged);
        let stale = home.bin_dir().join(format!("{HOLDER_PREFIX}old"));
        fs::write(&stale, b"old").unwrap();
        let running = home.bin_dir().join(format!("{HOLDER_PREFIX}running"));
        fs::write(&running, b"running").unwrap();
        fs::create_dir_all(home.agent_dir(4)).unwrap();
        let runtime = RuntimeInfo {
            holder_pid: 1,
            child_pid: None,
            port: 1,
            token: String::new(),
            started_at_ms: 1,
            protocol: 1,
            version: String::new(),
            exe: Some(PathBuf::from(running.to_string_lossy().replace('\\', "/"))),
        };
        fsutil::write_json_atomic(&home.runtime_file(4), &runtime).unwrap();
        prune_holder_binaries(&home, &staged);
        assert!(!stale.exists());
        assert!(staged.exists());
        assert!(running.exists(), "agents call their holder binary from hooks, so it must stay while they run");
        fs::remove_dir_all(dir).unwrap();
    }

    fn versioned_claude(dir: &Path, version: &str) -> PathBuf {
        let stem: String = version.chars().filter(char::is_ascii_alphanumeric).collect();
        if cfg!(windows) {
            let script = dir.join(format!("claude{stem}.cmd"));
            fs::write(&script, format!("@echo {version}\r\n")).unwrap();
            script
        } else {
            let script = dir.join(format!("claude{stem}"));
            fs::write(&script, format!("#!/bin/sh\necho '{version}'\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            }
            script
        }
    }

    fn setup(dir: &Path) -> HookSetup {
        HookSetup {
            exe: dir.join("bin").join("orkai-holder"),
            claude_settings: dir.join("claude-settings.json"),
            codex_home: Some(dir.join("codex-home")),
        }
    }

    fn claude_config(program: &Path, args: &[&str]) -> Config {
        Config {
            claude_command: program.to_string_lossy().into_owned(),
            claude_args: args.iter().map(|a| a.to_string()).collect(),
            ..Config::default()
        }
    }

    #[test]
    fn claude_gets_async_hooks_through_a_settings_file_when_its_version_supports_them() {
        let dir = temp_dir("claude-hooks");
        let program = versioned_claude(&dir, "2.1.302 (Claude Code)");
        let hooks = setup(&dir);
        let mut r = record(AgentKind::Claude, dir.clone(), "task");
        let spec = build_spec(
            &claude_config(&program, &[]),
            &mut r,
            LaunchMode::Fresh,
            TermSize::new(24, 80),
            None,
            Some(&hooks),
        )
        .unwrap();
        let settings_arg = hooks.claude_settings.to_string_lossy().into_owned();
        let tail: Vec<&str> = spec.args.iter().rev().take(2).rev().map(String::as_str).collect();
        assert_eq!(tail, vec!["--settings", settings_arg.as_str()]);
        assert!(spec.args.contains(&"--remote-control".to_string()));
        let written: serde_json::Value = fsutil::read_json(&hooks.claude_settings).unwrap();
        let command = written["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap().to_string();
        assert_eq!(command, hooks::claude_hook_command(&hooks.exe, r.id));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_claude_versions_and_user_settings_keep_launching_without_hooks() {
        let dir = temp_dir("claude-no-hooks");
        let hooks = setup(&dir);
        let old = versioned_claude(&dir, "2.1.77 (Claude Code)");
        let mut r = record(AgentKind::Claude, dir.clone(), "task");
        let spec =
            build_spec(&claude_config(&old, &[]), &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, Some(&hooks))
                .unwrap();
        assert!(!spec.args.contains(&"--settings".to_string()));
        assert!(!hooks.claude_settings.exists());

        let current = versioned_claude(&dir, "2.1.302 (Claude Code)");
        let own = claude_config(&current, &["--settings", "mine.json"]);
        let spec = build_spec(&own, &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, Some(&hooks)).unwrap();
        assert_eq!(spec.args.iter().filter(|a| *a == "--settings").count(), 1);
        assert!(spec.args.contains(&"mine.json".to_string()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_reports_finished_turns_through_notify_unless_one_is_configured() {
        let dir = temp_dir("codex-notify");
        let program = fake_program(&dir);
        let hooks = setup(&dir);
        let notify = hooks::codex_notify_override(&hooks.exe, 5);
        let mut r = record(AgentKind::Codex, dir.clone(), "task");
        let fresh =
            build_spec(&config_with(&program), &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, Some(&hooks))
                .unwrap();
        assert_eq!(fresh.args, vec!["-c".to_string(), notify.clone()]);
        r.conversation_id = Some("abc-123".into());
        let resumed =
            build_spec(&config_with(&program), &mut r, LaunchMode::Resume, TermSize::new(24, 80), None, Some(&hooks))
                .unwrap();
        assert_eq!(resumed.args, vec!["-c".to_string(), notify, "resume".into(), "abc-123".into()]);

        let own_args = Config { codex_args: vec!["-c".into(), "notify=[\"say\"]".into()], ..config_with(&program) };
        let spec = build_spec(&own_args, &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, Some(&hooks)).unwrap();
        assert_eq!(spec.args, vec!["-c", "notify=[\"say\"]"]);

        fs::create_dir_all(dir.join("codex-home")).unwrap();
        fs::write(dir.join("codex-home").join("config.toml"), "notify = [\"notify-send\", \"Codex\"]\n").unwrap();
        let spec =
            build_spec(&config_with(&program), &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, Some(&hooks))
                .unwrap();
        assert!(spec.args.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn terminals_never_get_hooks() {
        let dir = temp_dir("terminal-hooks");
        let program = fake_program(&dir);
        let hooks = setup(&dir);
        let mut r = record(AgentKind::Terminal, dir.clone(), "npm run dev");
        let spec =
            build_spec(&config_with(&program), &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, Some(&hooks))
                .unwrap();
        assert!(spec.args.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn batch_programs_launch_without_hooks_when_the_hook_arguments_are_unsafe_for_cmd() {
        let dir = temp_dir("batch-hooks");
        let program = dir.join("codex.cmd");
        fs::write(&program, b"@echo off\r\n").unwrap();
        let hooks = HookSetup { exe: PathBuf::from(r"C:\Users\me (work)\orkai-holder.exe"), ..setup(&dir) };
        let mut r = record(AgentKind::Codex, dir.clone(), "task");
        let spec =
            build_spec(&config_with(&program), &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, Some(&hooks))
                .unwrap();
        assert!(spec.program.to_string_lossy().to_ascii_lowercase().ends_with("cmd.exe"));
        assert!(!spec.args.iter().any(|a| a.starts_with("notify=")));
        let safe = setup(&dir);
        let spec =
            build_spec(&config_with(&program), &mut r, LaunchMode::Fresh, TermSize::new(24, 80), None, Some(&safe))
                .unwrap();
        assert!(spec.args.iter().any(|a| a.starts_with("notify=")));
        fs::remove_dir_all(dir).unwrap();
    }
}
