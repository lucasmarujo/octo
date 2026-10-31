use std::io;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::holder::nested;
use crate::paths::Home;

pub const HOST_COMMAND: &str = "__agents";
const CHECK_EVERY: Duration = Duration::from_secs(60);
const IDLE_CHECKS: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostInfo {
    pub pid: u32,
    pub created: u64,
}

pub fn run(home: &Home) -> io::Result<()> {
    let me = std::process::id();
    wait_until_alone(me, CHECK_EVERY, IDLE_CHECKS);
    if read(home).is_some_and(|info| info.pid == me) {
        let _ = std::fs::remove_file(home.agents_host_file());
    }
    Ok(())
}

pub fn wait_until_alone(root: u32, every: Duration, idle_checks: u32) {
    let mut idle = 0;
    while idle < idle_checks {
        thread::sleep(every);
        idle = if nested::descendants(root).is_empty() { idle + 1 } else { 0 };
    }
}

fn read(home: &Home) -> Option<HostInfo> {
    crate::fsutil::read_json_opt(&home.agents_host_file()).ok().flatten()
}

#[cfg(windows)]
pub use windows::spawn_holder;

#[cfg(windows)]
mod windows {
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString, c_void};
    use std::io;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::ptr;

    use super::{HOST_COMMAND, HostInfo, read};
    use crate::launch::{HOLDER_COMMAND, spawn_detached};
    use crate::model::AgentId;
    use crate::paths::{HOME_ENV, Home};

    type Handle = *mut c_void;

    const PROCESS_CREATE_PROCESS: u32 = 0x0080;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    const PROC_THREAD_ATTRIBUTE_PARENT_PROCESS: usize = 0x0002_0000;
    const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count_chars: u32,
        y_count_chars: u32,
        fill_attribute: u32,
        flags: u32,
        show_window: u16,
        reserved2_len: u16,
        reserved2: *mut u8,
        std_input: Handle,
        std_output: Handle,
        std_error: Handle,
    }

    #[repr(C)]
    struct StartupInfoExW {
        startup_info: StartupInfoW,
        attribute_list: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInformation {
        process: Handle,
        thread: Handle,
        process_id: u32,
        thread_id: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
        fn GetProcessTimes(
            process: Handle,
            creation: *mut u64,
            exit: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
        fn InitializeProcThreadAttributeList(list: *mut c_void, count: u32, flags: u32, size: *mut usize) -> i32;
        fn UpdateProcThreadAttribute(
            list: *mut c_void,
            flags: u32,
            attribute: usize,
            value: *const c_void,
            size: usize,
            previous: *mut c_void,
            returned: *mut usize,
        ) -> i32;
        fn DeleteProcThreadAttributeList(list: *mut c_void);
        fn CreateProcessW(
            application: *const u16,
            command_line: *mut u16,
            process_attributes: *const c_void,
            thread_attributes: *const c_void,
            inherit_handles: i32,
            flags: u32,
            environment: *const c_void,
            current_directory: *const u16,
            startup_info: *const StartupInfoW,
            information: *mut ProcessInformation,
        ) -> i32;
    }

    pub struct Process(Handle);

    impl Process {
        pub fn open(pid: u32) -> Option<Self> {
            let handle = unsafe { OpenProcess(PROCESS_CREATE_PROCESS | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            (!handle.is_null()).then_some(Self(handle))
        }

        pub fn alive(&self) -> bool {
            let mut code = 0;
            unsafe { GetExitCodeProcess(self.0, &mut code) != 0 && code == STILL_ACTIVE }
        }

        pub fn created(&self) -> Option<u64> {
            let (mut created, mut exited, mut kernel, mut user) = (0u64, 0u64, 0u64, 0u64);
            let ok = unsafe { GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user) };
            (ok != 0).then_some(created)
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn spawn_holder(home: &Home, exe: &Path, id: AgentId) -> io::Result<u32> {
        let host = current(home).map_or_else(|| start(home, exe), Ok)?;
        let id = id.to_string();
        let environment = environment_block(std::env::vars_os(), HOME_ENV, home.root().as_os_str());
        spawn_child(&host, exe, &[HOLDER_COMMAND, &id], &environment, home.root())
    }

    pub fn current(home: &Home) -> Option<Process> {
        let info = read(home)?;
        let process = Process::open(info.pid)?;
        (process.alive() && process.created() == Some(info.created)).then_some(process)
    }

    fn start(home: &Home, exe: &Path) -> io::Result<Process> {
        let mut command = Command::new(exe);
        command
            .arg(HOST_COMMAND)
            .env(HOME_ENV, home.root())
            .current_dir(home.root())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = spawn_detached(&mut command)?;
        let pid = child.id();
        let process = Process::open(pid).ok_or_else(io::Error::last_os_error)?;
        let created = process.created().ok_or_else(io::Error::last_os_error)?;
        crate::fsutil::write_json_atomic(&home.agents_host_file(), &HostInfo { pid, created })?;
        Ok(process)
    }

    pub fn spawn_child(
        parent: &Process,
        exe: &Path,
        args: &[&str],
        environment: &[u16],
        cwd: &Path,
    ) -> io::Result<u32> {
        let mut size = 0usize;
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
        let mut storage = vec![0usize; size.div_ceil(std::mem::size_of::<usize>()).max(1)];
        let list = storage.as_mut_ptr().cast::<c_void>();
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let parent_handle = parent.0;
        let updated = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS,
                (&raw const parent_handle).cast::<c_void>(),
                std::mem::size_of::<Handle>(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if updated == 0 {
            let err = io::Error::last_os_error();
            unsafe { DeleteProcThreadAttributeList(list) };
            return Err(err);
        }
        let startup = StartupInfoExW {
            startup_info: StartupInfoW {
                cb: u32::try_from(std::mem::size_of::<StartupInfoExW>()).unwrap_or(0),
                reserved: ptr::null_mut(),
                desktop: ptr::null_mut(),
                title: ptr::null_mut(),
                x: 0,
                y: 0,
                x_size: 0,
                y_size: 0,
                x_count_chars: 0,
                y_count_chars: 0,
                fill_attribute: 0,
                flags: 0,
                show_window: 0,
                reserved2_len: 0,
                reserved2: ptr::null_mut(),
                std_input: ptr::null_mut(),
                std_output: ptr::null_mut(),
                std_error: ptr::null_mut(),
            },
            attribute_list: list,
        };
        let mut line = command_line(exe, args);
        let directory: Vec<u16> = cwd.as_os_str().encode_wide().chain(iter::once(0)).collect();
        let mut information =
            ProcessInformation { process: ptr::null_mut(), thread: ptr::null_mut(), process_id: 0, thread_id: 0 };
        let flags =
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        let created = unsafe {
            CreateProcessW(
                ptr::null(),
                line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                flags,
                environment.as_ptr().cast::<c_void>(),
                directory.as_ptr(),
                &raw const startup.startup_info,
                &mut information,
            )
        };
        let err = io::Error::last_os_error();
        unsafe { DeleteProcThreadAttributeList(list) };
        if created == 0 {
            return Err(err);
        }
        unsafe {
            CloseHandle(information.thread);
            CloseHandle(information.process);
        }
        Ok(information.process_id)
    }

    pub fn quote(arg: &str) -> String {
        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            return arg.to_string();
        }
        let mut out = String::from('"');
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    out.extend(iter::repeat_n('\\', backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                }
                _ => {
                    out.extend(iter::repeat_n('\\', backslashes));
                    out.push(c);
                    backslashes = 0;
                }
            }
        }
        out.extend(iter::repeat_n('\\', backslashes * 2));
        out.push('"');
        out
    }

    pub fn command_line(exe: &Path, args: &[&str]) -> Vec<u16> {
        let mut line: Vec<u16> = iter::once(u16::from(b'"'))
            .chain(exe.as_os_str().encode_wide())
            .chain(iter::once(u16::from(b'"')))
            .collect();
        for arg in args {
            line.push(u16::from(b' '));
            line.extend(quote(arg).encode_utf16());
        }
        line.push(0);
        line
    }

    pub fn environment_block(
        vars: impl IntoIterator<Item = (OsString, OsString)>,
        name: &str,
        value: &OsStr,
    ) -> Vec<u16> {
        let mut sorted: BTreeMap<String, (OsString, OsString)> = vars
            .into_iter()
            .filter(|(key, _)| !key.is_empty() && !key.to_string_lossy().eq_ignore_ascii_case(name))
            .map(|(key, value)| (key.to_string_lossy().to_uppercase(), (key, value)))
            .collect();
        sorted.insert(name.to_uppercase(), (OsString::from(name), value.to_os_string()));
        let mut block = Vec::new();
        for (key, value) in sorted.into_values() {
            block.extend(key.encode_wide());
            block.push(u16::from(b'='));
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::path::Path;
    use std::process::Command;
    use std::time::Instant;

    use super::windows::{Process, command_line, current, environment_block, quote, spawn_child};
    use super::*;
    use crate::launch::{kill_process_tree, spawn_detached, without_console};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn ping(seconds: u32) -> std::process::Child {
        without_console(&mut Command::new("ping")).args(["-n", &seconds.to_string(), "127.0.0.1"]).spawn().unwrap()
    }

    #[test]
    fn arguments_are_quoted_the_way_windows_splits_them() {
        assert_eq!(quote("__holder"), "__holder");
        assert_eq!(quote("two words"), "\"two words\"");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(r"C:\dir with space\"), r#""C:\dir with space\\""#);
        let line = command_line(Path::new(r"C:\Program Files\Octo\octo.exe"), &["__holder", "7"]);
        assert_eq!(line, [wide(r#""C:\Program Files\Octo\octo.exe" __holder 7"#), vec![0]].concat());
    }

    #[test]
    fn the_holder_gets_the_callers_environment_with_its_home_set() {
        let vars = vec![
            (OsString::from("Path"), OsString::from(r"C:\bin")),
            (OsString::from("orkai_home"), OsString::from("old")),
            (OsString::from("APPDATA"), OsString::from(r"C:\app")),
        ];
        let block = environment_block(vars, "ORKAI_HOME", OsStr::new(r"C:\home"));
        let text = String::from_utf16(&block).unwrap();
        assert_eq!(text, "APPDATA=C:\\app\0ORKAI_HOME=C:\\home\0Path=C:\\bin\0\0");
        assert_eq!(
            environment_block(Vec::new(), "ORKAI_HOME", OsStr::new("h")),
            [wide("ORKAI_HOME=h"), vec![0, 0]].concat()
        );
    }

    #[test]
    fn a_child_spawned_under_the_host_is_its_child_and_not_ours() {
        let mut host = ping(30);
        let parent = Process::open(host.id()).unwrap();
        let environment = environment_block(std::env::vars_os(), "OCTO_TEST", OsStr::new("1"));
        let ping = std::env::var_os("SystemRoot").map(|root| Path::new(&root).join(r"System32\PING.EXE")).unwrap();
        let child =
            spawn_child(&parent, &ping, &["-n", "5", "127.0.0.1"], &environment, &std::env::temp_dir()).unwrap();
        let under_host: Vec<u32> = nested::descendants(host.id()).iter().map(|process| process.pid).collect();
        kill_process_tree(host.id());
        let _ = host.wait();
        assert!(under_host.contains(&child), "{child} is not a child of the host: {under_host:?}");
    }

    #[test]
    fn a_host_is_only_trusted_while_it_is_the_same_process_that_was_recorded() {
        let dir = std::env::temp_dir().join(format!("orkai-host-{}-{}", std::process::id(), crate::util::now_ms()));
        let home = Home::at(dir.clone());
        std::fs::create_dir_all(home.root()).unwrap();
        assert!(current(&home).is_none(), "no host recorded");
        let mut host = ping(30);
        let created = Process::open(host.id()).unwrap().created().unwrap();
        crate::fsutil::write_json_atomic(&home.agents_host_file(), &HostInfo { pid: host.id(), created: created + 1 })
            .unwrap();
        assert!(current(&home).is_none(), "same pid, another process");
        crate::fsutil::write_json_atomic(&home.agents_host_file(), &HostInfo { pid: host.id(), created }).unwrap();
        assert!(current(&home).is_some());
        kill_process_tree(host.id());
        let _ = host.wait();
        assert!(current(&home).is_none(), "a host that ended is not reused");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_host_stays_while_it_has_agents_and_leaves_when_alone() {
        let mut lonely = spawn_detached(Command::new("ping").args(["-n", "30", "127.0.0.1"])).unwrap();
        let started = Instant::now();
        wait_until_alone(lonely.id(), Duration::from_millis(30), 2);
        assert!(started.elapsed() < Duration::from_secs(2), "alone, it leaves at once");
        kill_process_tree(lonely.id());
        let _ = lonely.wait();
        let mut parent =
            without_console(&mut Command::new("cmd")).args(["/d", "/c", "ping -n 3 127.0.0.1 >nul"]).spawn().unwrap();
        let started = Instant::now();
        wait_until_alone(parent.id(), Duration::from_millis(100), 2);
        assert!(started.elapsed() >= Duration::from_millis(1500), "it waited for its child: {:?}", started.elapsed());
        let _ = parent.wait();
    }
}
