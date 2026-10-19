use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::fsutil;
use crate::model::{AgentKind, NestedAgent};
use crate::util::is_uuid;

pub const CHECK_EVERY_TICKS: u32 = 10;
pub const MAX_PROCESSES: usize = 256;
const CLAUDE_SESSIONS_DIR: &str = "sessions";
const QWEN_SESSIONS_DIR: &str = "sessions";
const PROGRAMS: [(&str, AgentKind); 9] = [
    ("claude", AgentKind::Claude),
    ("codex", AgentKind::Codex),
    ("kilo", AgentKind::Kilo),
    ("kilocode", AgentKind::Kilo),
    ("opencode", AgentKind::Opencode),
    ("agy", AgentKind::Antigravity),
    ("grok", AgentKind::Grok),
    ("qwen", AgentKind::Qwen),
    ("kimi", AgentKind::Kimi),
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
