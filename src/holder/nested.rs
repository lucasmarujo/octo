use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::fsutil;
use crate::model::{AgentKind, NestedAgent, SavedNested};
use crate::util::is_uuid;

pub const CHECK_EVERY_TICKS: u32 = 10;
pub const MAX_PROCESSES: usize = 256;
pub const GONE_GRACE_MS: u64 = 60_000;
const MAX_COMMAND_ARGS: usize = 128;
const MAX_COMMAND_BYTES: usize = 16 * 1024;
const CLAUDE_SESSIONS_DIR: &str = "sessions";
const QWEN_SESSIONS_DIR: &str = "sessions";
const CLAUDE_BUSY_STATUS: &str = "busy";
const PROGRAMS: [(&str, AgentKind); 11] = [
    ("claude", AgentKind::Claude),
    ("codex", AgentKind::Codex),
    ("kilo", AgentKind::Kilo),
    ("kilocode", AgentKind::Kilo),
    ("opencode", AgentKind::Opencode),
    ("agy", AgentKind::Antigravity),
    ("grok", AgentKind::Grok),
    ("qwen", AgentKind::Qwen),
    ("kimi", AgentKind::Kimi),
    ("pi", AgentKind::Pi),
    ("cursor-agent", AgentKind::Cursor),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub name: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SessionFile {
    pid: u32,
    session_id: String,
    cwd: Option<PathBuf>,
    status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeSession {
    pub session_id: String,
    pub cwd: Option<PathBuf>,
    pub busy: bool,
}

pub fn program_of(kind: AgentKind) -> Option<&'static str> {
    PROGRAMS.iter().find(|(_, program)| *program == kind).map(|(name, _)| *name)
}

pub fn program_name(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    match file.strip_suffix(".exe") {
        Some(stem) => stem.to_string(),
        None => file,
    }
}

fn session_file(dir: &Path, pid: u32) -> Option<SessionFile> {
    let session: SessionFile = fsutil::read_json_opt(&dir.join(format!("{pid}.json"))).ok().flatten()?;
    (session.pid == pid && is_uuid(&session.session_id)).then_some(session)
}

pub fn claude_session(claude_home: &Path, pid: u32) -> Option<String> {
    session_file(&claude_home.join(CLAUDE_SESSIONS_DIR), pid).map(|session| session.session_id)
}

pub fn claude_sessions_among(claude_home: &Path, processes: &[Process]) -> Vec<ClaudeSession> {
    let dir = claude_home.join(CLAUDE_SESSIONS_DIR);
    processes
        .iter()
        .filter_map(|process| session_file(&dir, process.pid))
        .map(|session| ClaudeSession {
            busy: session.status.as_deref() == Some(CLAUDE_BUSY_STATUS),
            session_id: session.session_id,
            cwd: session.cwd,
        })
        .collect()
}

pub fn detect(
    processes: &[Process],
    claude_home: Option<&Path>,
    qwen_home: Option<&Path>,
    now: u64,
) -> Option<NestedAgent> {
    let registries = [
        (AgentKind::Claude, claude_home.map(|home| home.join(CLAUDE_SESSIONS_DIR))),
        (AgentKind::Qwen, qwen_home.map(|home| home.join(QWEN_SESSIONS_DIR))),
    ];
    processes.iter().find_map(|process| {
        for (kind, dir) in &registries {
            if let Some(session) = dir.as_deref().and_then(|dir| session_file(dir, process.pid)) {
                return Some(NestedAgent {
                    kind: *kind,
                    pid: process.pid,
                    conversation_id: Some(session.session_id),
                    cwd: session.cwd,
                    since_ms: now,
                });
            }
        }
        let name = program_name(&process.name);
        let kind = PROGRAMS.iter().find(|(program, _)| *program == name)?.1;
        Some(NestedAgent { kind, pid: process.pid, conversation_id: None, cwd: None, since_ms: now })
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Kept {
    Save(SavedNested),
    Forget,
}

pub struct Keeper {
    saved: Option<SavedNested>,
    inherited: Option<(AgentKind, String)>,
    running: Option<SavedNested>,
    gone_since: Option<u64>,
}

impl Keeper {
    pub fn new(previous: Option<SavedNested>) -> Self {
        let inherited =
            previous.as_ref().and_then(|saved| Some((saved.agent.kind, saved.agent.conversation_id.clone()?)));
        Self { saved: previous, inherited, running: None, gone_since: None }
    }

    pub fn observe(
        &mut self,
        found: Option<NestedAgent>,
        now: u64,
        command_line: impl FnOnce(u32) -> Vec<String>,
    ) -> (Option<NestedAgent>, Option<Kept>) {
        let Some(mut agent) = found else {
            if self.running.is_none() {
                return (None, None);
            }
            let gone_since = *self.gone_since.get_or_insert(now);
            if now.saturating_sub(gone_since) < GONE_GRACE_MS {
                return (None, None);
            }
            self.running = None;
            self.gone_since = None;
            return (None, self.saved.take().map(|_| Kept::Forget));
        };
        self.gone_since = None;
        let (command, known) = match self.running.take().filter(|running| running.agent.pid == agent.pid) {
            Some(running) => {
                agent.since_ms = running.agent.since_ms;
                (running.command, running.agent.conversation_id)
            }
            None => {
                let inherited = self.inherited.take().filter(|(kind, _)| *kind == agent.kind).map(|(_, id)| id);
                (bounded(command_line(agent.pid)), inherited)
            }
        };
        if agent.conversation_id.is_none() {
            agent.conversation_id = known;
        }
        let running = SavedNested { agent: agent.clone(), command };
        let change = (self.saved.as_ref() != Some(&running)).then(|| Kept::Save(running.clone()));
        if change.is_some() {
            self.saved = Some(running.clone());
        }
        self.running = Some(running);
        (Some(agent), change)
    }
}

fn bounded(command: Vec<String>) -> Vec<String> {
    let bytes: usize = command.iter().map(String::len).sum();
    if command.len() > MAX_COMMAND_ARGS || bytes > MAX_COMMAND_BYTES { Vec::new() } else { command }
}

#[cfg(windows)]
pub fn command_line(pid: u32) -> Vec<String> {
    use std::ffi::c_void;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    const BUFFER_WORDS: usize = 9 * 1024;
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *const u16,
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(
            process: *mut c_void,
            class: u32,
            information: *mut c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn CommandLineToArgvW(command_line: *const u16, count: *mut i32) -> *mut *mut u16;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Vec::new();
    }
    let mut buffer = vec![0u64; BUFFER_WORDS];
    let bytes = u32::try_from(std::mem::size_of_val(buffer.as_slice())).unwrap_or(0);
    let mut returned = 0u32;
    let status = unsafe {
        NtQueryInformationProcess(
            process,
            PROCESS_COMMAND_LINE_INFORMATION,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut returned,
        )
    };
    unsafe { CloseHandle(process) };
    if status < 0 {
        return Vec::new();
    }
    let text = unsafe { &*buffer.as_ptr().cast::<UnicodeString>() };
    let units = usize::from(text.length) / std::mem::size_of::<u16>();
    if text.buffer.is_null() || units == 0 {
        return Vec::new();
    }
    let mut line: Vec<u16> = unsafe { std::slice::from_raw_parts(text.buffer, units) }.to_vec();
    line.push(0);
    let mut count = 0i32;
    let parsed = unsafe { CommandLineToArgvW(line.as_ptr(), &mut count) };
    if parsed.is_null() {
        return Vec::new();
    }
    let args = (0..usize::try_from(count).unwrap_or(0))
        .map(|index| {
            let arg = unsafe { *parsed.add(index) };
            let len = (0..).take_while(|offset| unsafe { *arg.add(*offset) } != 0).count();
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(arg, len) })
        })
        .collect();
    unsafe { LocalFree(parsed.cast()) };
    args
}

#[cfg(target_os = "linux")]
pub fn command_line(pid: u32) -> Vec<String> {
    let Ok(bytes) = std::fs::read(format!("/proc/{pid}/cmdline")) else { return Vec::new() };
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(&bytes);
    if bytes.is_empty() {
        return Vec::new();
    }
    bytes.split(|byte| *byte == 0).map(|arg| String::from_utf8_lossy(arg).into_owned()).collect()
}

#[cfg(any(target_os = "macos", test))]
fn parse_procargs(bytes: &[u8]) -> Vec<String> {
    const COUNT_BYTES: usize = std::mem::size_of::<i32>();
    let Some((count, rest)) = bytes.split_at_checked(COUNT_BYTES) else { return Vec::new() };
    let count = usize::try_from(i32::from_ne_bytes([count[0], count[1], count[2], count[3]])).unwrap_or(0);
    let Some(path_end) = rest.iter().position(|byte| *byte == 0) else { return Vec::new() };
    let after_path = &rest[path_end..];
    let Some(first) = after_path.iter().position(|byte| *byte != 0) else { return Vec::new() };
    let args: Vec<String> = after_path[first..]
        .split(|byte| *byte == 0)
        .take(count)
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect();
    if args.len() == count { args } else { Vec::new() }
}

#[cfg(target_os = "macos")]
pub fn command_line(pid: u32) -> Vec<String> {
    let Ok(pid) = libc::c_int::try_from(pid) else { return Vec::new() };
    let mut limit: libc::c_int = 0;
    let mut limit_size = std::mem::size_of::<libc::c_int>();
    let mut limit_name = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let asked = unsafe {
        libc::sysctl(
            limit_name.as_mut_ptr(),
            2,
            std::ptr::from_mut(&mut limit).cast(),
            &mut limit_size,
            std::ptr::null_mut(),
            0,
        )
    };
    let Ok(mut size) = usize::try_from(limit) else { return Vec::new() };
    if asked != 0 || size == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u8; size];
    let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let read =
        unsafe { libc::sysctl(name.as_mut_ptr(), 3, buffer.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) };
    if read != 0 {
        return Vec::new();
    }
    buffer.truncate(size);
    parse_procargs(&buffer)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn command_line(_pid: u32) -> Vec<String> {
    Vec::new()
}

pub fn walk(root: u32, mut children: impl FnMut(u32) -> Vec<Process>) -> Vec<Process> {
    let mut found = Vec::new();
    let mut seen = HashSet::from([root]);
    let mut queue = VecDeque::from([root]);
    while let Some(pid) = queue.pop_front() {
        for child in children(pid) {
            if found.len() >= MAX_PROCESSES {
                return found;
            }
            if seen.insert(child.pid) {
                queue.push_back(child.pid);
                found.push(child);
            }
        }
    }
    found
}

#[cfg(windows)]
pub fn descendants(root: u32) -> Vec<Process> {
    use std::collections::HashMap;
    use std::ffi::c_void;
    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const MAX_PATH: usize = 260;
    #[repr(C)]
    struct ProcessEntry32W {
        size: u32,
        usage: u32,
        process_id: u32,
        default_heap_id: usize,
        module_id: u32,
        threads: u32,
        parent_process_id: u32,
        priority_class_base: i32,
        flags: u32,
        exe_file: [u16; MAX_PATH],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() || snapshot as isize == -1 {
        return Vec::new();
    }
    let mut entry = ProcessEntry32W {
        size: u32::try_from(std::mem::size_of::<ProcessEntry32W>()).unwrap_or(0),
        usage: 0,
        process_id: 0,
        default_heap_id: 0,
        module_id: 0,
        threads: 0,
        parent_process_id: 0,
        priority_class_base: 0,
        flags: 0,
        exe_file: [0; MAX_PATH],
    };
    let mut children: HashMap<u32, Vec<Process>> = HashMap::new();
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        let len = entry.exe_file.iter().position(|c| *c == 0).unwrap_or(MAX_PATH);
        let name = String::from_utf16_lossy(&entry.exe_file[..len]);
        if entry.process_id != entry.parent_process_id {
            children.entry(entry.parent_process_id).or_default().push(Process { pid: entry.process_id, name });
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    walk(root, |pid| children.remove(&pid).unwrap_or_default())
}

#[cfg(target_os = "linux")]
fn linux_name(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|name| name.trim().to_string()).unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn linux_children(pid: u32) -> Option<Vec<Process>> {
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).ok()?;
    let mut pids = Vec::new();
    let mut supported = false;
    for task in tasks.flatten() {
        if let Ok(text) = std::fs::read_to_string(task.path().join("children")) {
            supported = true;
            pids.extend(text.split_whitespace().filter_map(|p| p.parse::<u32>().ok()));
        }
    }
    supported.then(|| pids.into_iter().map(|pid| Process { pid, name: linux_name(pid) }).collect())
}

#[cfg(target_os = "linux")]
fn linux_parents() -> std::collections::HashMap<u32, Vec<Process>> {
    let mut children: std::collections::HashMap<u32, Vec<Process>> = std::collections::HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else { return children };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else { continue };
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else { continue };
        let Some(ppid) = stat[close + 1..].split_whitespace().nth(1).and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        children.entry(ppid).or_default().push(Process { pid, name: stat[open + 1..close].to_string() });
    }
    children
}

#[cfg(target_os = "linux")]
pub fn descendants(root: u32) -> Vec<Process> {
    if linux_children(root).is_some() {
        return walk(root, |pid| linux_children(pid).unwrap_or_default());
    }
    let mut children = linux_parents();
    walk(root, |pid| children.remove(&pid).unwrap_or_default())
}

#[cfg(target_os = "macos")]
fn macos_name(pid: libc::c_int) -> String {
    let mut buffer = [0u8; 256];
    let size = u32::try_from(buffer.len()).unwrap_or(0);
    let len = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), size) };
    let len = usize::try_from(len).unwrap_or(0).min(buffer.len());
    String::from_utf8_lossy(&buffer[..len]).into_owned()
}

#[cfg(target_os = "macos")]
pub fn descendants(root: u32) -> Vec<Process> {
    walk(root, |pid| {
        let Ok(pid) = libc::pid_t::try_from(pid) else { return Vec::new() };
        let mut pids: Vec<libc::pid_t> = vec![0; MAX_PROCESSES];
        let size = libc::c_int::try_from(pids.len() * std::mem::size_of::<libc::pid_t>()).unwrap_or(0);
        let count = unsafe { libc::proc_listchildpids(pid, pids.as_mut_ptr().cast(), size) };
        pids.truncate(usize::try_from(count).unwrap_or(0).min(MAX_PROCESSES));
        pids.into_iter()
            .filter(|child| *child > 0)
            .filter_map(|child| Some(Process { pid: u32::try_from(child).ok()?, name: macos_name(child) }))
            .collect()
    })
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn descendants(_root: u32) -> Vec<Process> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    const SESSION: &str = "919fa459-6c5f-4786-bba1-2f77803f6868";

    fn process(pid: u32, name: &str) -> Process {
        Process { pid, name: name.to_string() }
    }

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn claude_home(sessions: &[(u32, &str)]) -> Dir {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "orkai-nested-{}-{}-{}",
            std::process::id(),
            crate::util::now_ms(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join(CLAUDE_SESSIONS_DIR)).unwrap();
        for (pid, json) in sessions {
            std::fs::write(dir.join(CLAUDE_SESSIONS_DIR).join(format!("{pid}.json")), json).unwrap();
        }
        Dir(dir)
    }

    #[test]
    fn program_names_ignore_paths_case_and_exe() {
        assert_eq!(program_name(r"C:\Users\me\.local\bin\Claude.EXE"), "claude");
        assert_eq!(program_name("/opt/homebrew/bin/codex"), "codex");
        assert_eq!(program_name("node.exe"), "node");
        assert_eq!(program_name("codex-exec"), "codex-exec");
    }

    #[test]
    fn claude_is_found_through_its_session_file_whatever_the_process_name() {
        let session = format!(r#"{{"pid":12,"sessionId":"{SESSION}","cwd":"/work/app","kind":"interactive"}}"#);
        let home = claude_home(&[(12, &session)]);
        let processes = [process(11, "cmd.exe"), process(12, "node.exe"), process(13, "codex.exe")];
        let found = detect(&processes, Some(&home.0), None, 500).unwrap();
        assert_eq!(found.kind, AgentKind::Claude);
        assert_eq!(found.pid, 12);
        assert_eq!(found.conversation_id.as_deref(), Some(SESSION));
        assert_eq!(found.cwd, Some(PathBuf::from("/work/app")));
        assert_eq!(found.since_ms, 500);
    }

    #[test]
    fn the_background_sessions_of_claude_come_from_the_session_files_of_its_processes() {
        const BACKGROUND: &str = "c82c00b5-5f5b-44d0-a322-f806f88bf804";
        let spare = format!(r#"{{"pid":3042,"sessionId":"{BACKGROUND}","cwd":"/work/app","status":"busy"}}"#);
        let idle = format!(r#"{{"pid":3047,"sessionId":"{SESSION}","status":"idle"}}"#);
        let stale = format!(r#"{{"pid":9999,"sessionId":"{SESSION}"}}"#);
        let home =
            claude_home(&[(3042, &spare), (3047, &idle), (3059, &stale), (3060, r#"{"pid":3060,"sessionId":"x"}"#)]);
        let processes = [
            process(3034, "claude"),
            process(3039, "claude"),
            process(3042, "claude"),
            process(3047, "claude"),
            process(3059, "claude"),
            process(3060, "claude"),
        ];
        let sessions = claude_sessions_among(&home.0, &processes);
        assert_eq!(
            sessions,
            [
                ClaudeSession { session_id: BACKGROUND.into(), cwd: Some(PathBuf::from("/work/app")), busy: true },
                ClaudeSession { session_id: SESSION.into(), cwd: None, busy: false },
            ],
            "a file of another process or without a session id is skipped"
        );
        assert!(claude_sessions_among(&home.0, &[]).is_empty());
    }

    #[test]
    fn programs_are_recognized_by_name_when_there_is_no_session_file() {
        let home = claude_home(&[]);
        let codex = detect(&[process(5, "bash"), process(6, "codex")], Some(&home.0), None, 1).unwrap();
        assert_eq!((codex.kind, codex.pid, codex.conversation_id), (AgentKind::Codex, 6, None));
        let claude = detect(&[process(7, "claude.exe")], None, None, 1).unwrap();
        assert_eq!((claude.kind, claude.pid), (AgentKind::Claude, 7));
        assert_eq!(detect(&[process(8, "npm"), process(9, "node")], Some(&home.0), Some(&home.0), 1), None);
        assert_eq!(detect(&[], Some(&home.0), None, 1), None);
        for (name, kind) in [
            ("kilo.exe", AgentKind::Kilo),
            ("kilocode", AgentKind::Kilo),
            ("opencode.exe", AgentKind::Opencode),
            ("agy.exe", AgentKind::Antigravity),
            ("/home/me/.grok/bin/grok", AgentKind::Grok),
            ("qwen", AgentKind::Qwen),
            ("kimi.exe", AgentKind::Kimi),
            ("/home/me/.local/bin/pi", AgentKind::Pi),
            ("/home/me/.local/bin/cursor-agent", AgentKind::Cursor),
        ] {
            assert_eq!(
                detect(&[process(3, "pwsh.exe"), process(4, name)], None, None, 1).unwrap().kind,
                kind,
                "{name}"
            );
        }
    }

    #[test]
    fn qwen_running_on_node_is_found_through_its_session_registry() {
        let session =
            format!(r#"{{"pid":31,"sessionId":"{SESSION}","cwd":"/work/api","name":"qwen","kind":"interactive"}}"#);
        let claude = claude_home(&[]);
        let qwen = claude_home(&[(31, &session)]);
        let processes = [process(30, "node.exe"), process(31, "node.exe")];
        let found = detect(&processes, Some(&claude.0), Some(&qwen.0), 9).unwrap();
        assert_eq!((found.kind, found.pid), (AgentKind::Qwen, 31));
        assert_eq!(found.conversation_id.as_deref(), Some(SESSION));
        assert_eq!(found.cwd, Some(PathBuf::from("/work/api")));
        assert_eq!(detect(&processes, Some(&claude.0), None, 9), None);
    }

    #[test]
    fn stale_or_foreign_session_files_are_ignored() {
        let home = claude_home(&[
            (20, &format!(r#"{{"pid":99,"sessionId":"{SESSION}"}}"#)),
            (21, r#"{"pid":21,"sessionId":"../../escape"}"#),
            (22, "not json"),
        ]);
        let processes = [process(20, "node"), process(21, "node"), process(22, "node")];
        assert_eq!(detect(&processes, Some(&home.0), Some(&home.0), 1), None);
    }

    #[test]
    fn the_closest_agent_to_the_shell_wins() {
        let tree: HashMap<u32, Vec<Process>> = HashMap::from([
            (1, vec![process(2, "codex"), process(3, "vim")]),
            (2, vec![process(4, "claude")]),
            (4, vec![process(1, "loop")]),
        ]);
        let processes = walk(1, |pid| tree.get(&pid).cloned().unwrap_or_default());
        assert_eq!(processes.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![2, 3, 4]);
        assert_eq!(detect(&processes, None, None, 1).unwrap().kind, AgentKind::Codex);
    }

    fn seen(kind: AgentKind, pid: u32, conversation: Option<&str>, now: u64) -> Option<NestedAgent> {
        Some(NestedAgent { kind, pid, conversation_id: conversation.map(str::to_string), cwd: None, since_ms: now })
    }

    fn command_of(pid: u32) -> Vec<String> {
        vec!["claude".to_string(), format!("--pid-{pid}")]
    }

    fn never_read(pid: u32) -> Vec<String> {
        panic!("the command line of {pid} was read again")
    }

    #[test]
    fn an_agent_started_in_a_terminal_is_kept_with_the_command_that_started_it() {
        let mut keeper = Keeper::new(None);
        assert_eq!(keeper.observe(None, 100, never_read), (None, None), "nothing ran yet");

        let (shown, change) = keeper.observe(seen(AgentKind::Claude, 42, None, 1_000), 1_000, command_of);
        let expected =
            SavedNested { agent: seen(AgentKind::Claude, 42, None, 1_000).unwrap(), command: command_of(42) };
        assert_eq!(shown, Some(expected.agent.clone()));
        assert_eq!(change, Some(Kept::Save(expected)));

        let (shown, change) = keeper.observe(seen(AgentKind::Claude, 42, None, 3_000), 3_000, never_read);
        assert_eq!(shown.unwrap().since_ms, 1_000, "the same process keeps the moment it was first seen");
        assert_eq!(change, None, "nothing changed, nothing is written");

        let (shown, change) = keeper.observe(seen(AgentKind::Claude, 42, Some(SESSION), 5_000), 5_000, never_read);
        assert_eq!(shown.as_ref().unwrap().conversation_id.as_deref(), Some(SESSION));
        let Some(Kept::Save(saved)) = change else { panic!("the conversation it opened must be kept") };
        assert_eq!((saved.agent.since_ms, saved.command), (1_000, command_of(42)));
    }

    #[test]
    fn an_agent_that_vanished_is_only_forgotten_after_the_terminal_outlives_it() {
        let mut keeper = Keeper::new(None);
        keeper.observe(seen(AgentKind::Claude, 42, Some(SESSION), 1_000), 1_000, command_of);
        assert_eq!(
            keeper.observe(None, 2_000, never_read),
            (None, None),
            "a shutdown kills it just before the terminal"
        );
        assert_eq!(keeper.observe(None, 2_000 + GONE_GRACE_MS - 1, never_read), (None, None));
        assert_eq!(keeper.observe(None, 2_000 + GONE_GRACE_MS, never_read), (None, Some(Kept::Forget)));
        assert_eq!(keeper.observe(None, 3_000 + GONE_GRACE_MS, never_read), (None, None), "forgotten once");

        keeper.observe(seen(AgentKind::Codex, 50, None, 200_000), 200_000, command_of);
        assert_eq!(keeper.observe(None, 201_000, never_read), (None, None));
        let (_, change) = keeper.observe(seen(AgentKind::Codex, 51, None, 202_000), 202_000, command_of);
        let Some(Kept::Save(saved)) = change else { panic!("another process is another agent") };
        assert_eq!((saved.agent.pid, saved.agent.since_ms, saved.command), (51, 202_000, command_of(51)));
        assert_eq!(keeper.observe(None, 203_000, never_read), (None, None), "the wait starts over");
        assert_eq!(keeper.observe(None, 202_500 + GONE_GRACE_MS, never_read), (None, None));
    }

    #[test]
    fn a_terminal_brought_back_keeps_what_ran_in_it_until_an_agent_shows_up_again() {
        let previous =
            SavedNested { agent: seen(AgentKind::Codex, 7, Some("thread-1"), 10).unwrap(), command: command_of(7) };
        let mut keeper = Keeper::new(Some(previous.clone()));
        assert_eq!(keeper.observe(None, 10 + 10 * GONE_GRACE_MS, never_read), (None, None), "it may still be starting");

        let (shown, change) = keeper.observe(seen(AgentKind::Codex, 90, None, 900_000), 900_000, command_of);
        assert_eq!(shown.unwrap().conversation_id.as_deref(), Some("thread-1"), "the resumed agent is on that thread");
        let Some(Kept::Save(saved)) = change else { panic!("the new process must be kept") };
        assert_eq!((saved.agent.pid, saved.agent.conversation_id.as_deref()), (90, Some("thread-1")));

        keeper.observe(None, 901_000, never_read);
        let (shown, _) = keeper.observe(seen(AgentKind::Codex, 91, None, 902_000), 902_000, command_of);
        assert_eq!(shown.unwrap().conversation_id, None, "a second agent started by hand is a new conversation");

        let mut other = Keeper::new(Some(previous));
        let (shown, _) = other.observe(seen(AgentKind::Kimi, 92, None, 5), 5, command_of);
        assert_eq!(shown.unwrap().conversation_id, None, "another agent does not take the thread of codex");
    }

    #[test]
    fn a_command_line_too_big_to_type_back_is_not_kept() {
        let mut keeper = Keeper::new(None);
        let huge = |_| vec!["x".repeat(MAX_COMMAND_BYTES + 1)];
        let (_, change) = keeper.observe(seen(AgentKind::Claude, 1, None, 1), 1, huge);
        let Some(Kept::Save(saved)) = change else { panic!("the agent itself is still kept") };
        assert!(saved.command.is_empty());
        assert!(bounded(vec!["a".to_string(); MAX_COMMAND_ARGS + 1]).is_empty());
        assert_eq!(bounded(command_of(3)), command_of(3));
    }

    #[test]
    fn the_arguments_of_a_mac_process_come_after_its_path_and_before_its_environment() {
        let mut bytes = 3i32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/opt/bin/claude\0\0\0\0claude\0--model\0opus 4\0HOME=/Users/me\0");
        assert_eq!(parse_procargs(&bytes), ["claude", "--model", "opus 4"]);
        assert!(parse_procargs(&bytes[..2]).is_empty());
        assert!(parse_procargs(&3i32.to_ne_bytes()).is_empty());
        let mut short = 5i32.to_ne_bytes().to_vec();
        short.extend_from_slice(b"/bin/x\0x\0");
        assert!(parse_procargs(&short).is_empty(), "fewer arguments than announced");
    }

    #[test]
    fn the_command_line_of_a_real_process_is_read_with_its_arguments() {
        let mut child = if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "30", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
        } else {
            std::process::Command::new("sleep").arg("30").spawn()
        }
        .unwrap();
        let expected: &[&str] = if cfg!(windows) { &["-n", "30", "127.0.0.1"] } else { &["30"] };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let command = loop {
            let command = command_line(child.id());
            if command.len() > expected.len() || std::time::Instant::now() >= deadline {
                break command;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(program_name(&command[0]), if cfg!(windows) { "ping" } else { "sleep" }, "{command:?}");
        assert_eq!(&command[1..], expected);
        assert!(command_line(u32::MAX - 1).is_empty(), "a process that does not exist");
    }

    #[test]
    fn each_agent_has_a_program_to_be_started_with() {
        assert_eq!(program_of(AgentKind::Claude), Some("claude"));
        assert_eq!(program_of(AgentKind::Kilo), Some("kilo"));
        assert_eq!(program_of(AgentKind::Cursor), Some("cursor-agent"));
        assert_eq!(program_of(AgentKind::Terminal), None);
        for kind in AgentKind::ALL.into_iter().filter(|kind| kind.is_ai()) {
            assert!(program_of(kind).is_some(), "{kind:?}");
        }
    }

    #[test]
    fn walks_are_bounded() {
        let mut next = 1;
        let processes = walk(0, |_| {
            next += 1;
            vec![process(next, "x")]
        });
        assert_eq!(processes.len(), MAX_PROCESSES);
    }

    #[test]
    fn real_child_processes_are_listed_as_descendants() {
        let mut child = if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "30", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
        } else {
            std::process::Command::new("sleep").arg("30").spawn()
        }
        .unwrap();
        let pid = child.id();
        let expected = if cfg!(windows) { "ping" } else { "sleep" };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let entry = loop {
            let entry = descendants(std::process::id()).into_iter().find(|p| p.pid == pid);
            let started = entry.as_ref().is_some_and(|p| program_name(&p.name) == expected);
            if started || std::time::Instant::now() >= deadline {
                break entry;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let _ = child.kill();
        let _ = child.wait();
        let entry = entry.unwrap_or_else(|| panic!("{pid} is not a descendant"));
        assert_eq!(program_name(&entry.name), expected);
    }
}
