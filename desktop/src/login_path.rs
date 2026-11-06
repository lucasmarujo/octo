use std::env;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MARK: &str = "__OCTO_PATH__";
const TIMEOUT: Duration = Duration::from_secs(3);
const FALLBACK_SHELL: &str = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" };

pub fn adopt() {
    if !cfg!(unix) || env::var_os("TERM").is_some() {
        return;
    }
    let shell = env::var_os("SHELL").filter(|shell| !shell.is_empty()).unwrap_or_else(|| FALLBACK_SHELL.into());
    let Some(login) = read(Path::new(&shell), TIMEOUT) else {
        return;
    };
    let current = env::var_os("PATH").unwrap_or_default();
    if let Some(path) = merge(&login, &current).filter(|path| *path != current) {
        unsafe {
            env::set_var("PATH", path);
        }
    }
}

#[cfg(unix)]
pub fn refresh() {
    adopt();
}

#[cfg(windows)]
pub fn refresh() {
    let stored: Vec<OsString> = registry::paths().into_iter().flatten().collect();
    let current = env::var_os("PATH").unwrap_or_default();
    if let Some(path) = with_new_entries(&current, &stored) {
        unsafe {
            env::set_var("PATH", path);
        }
    }
}

#[cfg(windows)]
fn with_new_entries(current: &OsStr, stored: &[OsString]) -> Option<OsString> {
    use orkai::paths::normalize_for_compare;

    let mut entries: Vec<PathBuf> = env::split_paths(current).collect();
    let mut known: Vec<String> = entries.iter().map(|entry| normalize_for_compare(entry)).collect();
    let before = entries.len();
    for entry in stored.iter().flat_map(env::split_paths).filter(|entry| entry.is_absolute()) {
        let key = normalize_for_compare(&entry);
        if !known.contains(&key) {
            known.push(key);
            entries.push(entry);
        }
    }
    if entries.len() == before { None } else { env::join_paths(entries).ok() }
}

#[cfg(windows)]
mod registry {
    use std::ffi::{OsStr, OsString, c_void};
    use std::iter;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr;

    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as i32 as isize;
    const USER_ENVIRONMENT: &str = "Environment";
    const MACHINE_ENVIRONMENT: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";
    const PATH_VALUE: &str = "Path";
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;
    const ERROR_SUCCESS: i32 = 0;
    const ERROR_MORE_DATA: i32 = 234;
    const READ_TRIES: usize = 3;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain(iter::once(0)).collect()
    }

    fn read(key: isize, subkey: &str) -> Option<OsString> {
        let (subkey, value) = (wide(subkey), wide(PATH_VALUE));
        let mut size = 0u32;
        for _ in 0..READ_TRIES {
            let found = unsafe {
                RegGetValueW(
                    key,
                    subkey.as_ptr(),
                    value.as_ptr(),
                    RRF_RT_REG_SZ,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut size,
                )
            };
            if found != ERROR_SUCCESS {
                return None;
            }
            let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
            let read = unsafe {
                RegGetValueW(
                    key,
                    subkey.as_ptr(),
                    value.as_ptr(),
                    RRF_RT_REG_SZ,
                    ptr::null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            match read {
                ERROR_SUCCESS => {
                    let text = buffer.split(|unit| *unit == 0).next().unwrap_or_default();
                    return Some(OsString::from_wide(text));
                }
                ERROR_MORE_DATA => {}
                _ => return None,
            }
        }
        None
    }

    pub fn paths() -> [Option<OsString>; 2] {
        [read(HKEY_LOCAL_MACHINE, MACHINE_ENVIRONMENT), read(HKEY_CURRENT_USER, USER_ENVIRONMENT)]
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_machine_path_is_read_from_the_registry_with_its_variables_expanded() {
            let machine = paths()[0].clone().expect("every Windows has a machine PATH");
            let text = machine.to_string_lossy().to_lowercase();
            assert!(text.contains(r"\system32"), "{text}");
            assert!(!text.contains('%'), "{text}");
            assert_eq!(read(HKEY_LOCAL_MACHINE, "SYSTEM\\NoSuchKey\\ForOcto"), None);
        }
    }
}

fn read(shell: &Path, timeout: Duration) -> Option<OsString> {
    let mut child = Command::new(shell)
        .args(["-ilc", &format!("printf '%s' \"{MARK}:$PATH:{MARK}\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (done, output) = mpsc::channel();
    thread::spawn(move || {
        let mut text = Vec::new();
        let mut chunk = [0; 4096];
        while let Ok(count) = stdout.read(&mut chunk) {
            if count == 0 {
                break;
            }
            text.extend_from_slice(&chunk[..count]);
            if String::from_utf8_lossy(&text).matches(MARK).count() >= 2 {
                break;
            }
        }
        let _ = done.send(String::from_utf8_lossy(&text).into_owned());
    });
    let text = output.recv_timeout(timeout).ok();
    let _ = child.kill();
    let _ = child.wait();
    parse(&text?).map(OsString::from)
}

fn parse(output: &str) -> Option<&str> {
    let start = output.find(MARK)? + MARK.len();
    let end = output.rfind(MARK)?;
    let path = output.get(start..end)?.trim_matches(|c: char| c == ':' || c.is_whitespace());
    (!path.is_empty()).then_some(path)
}

fn merge(login: &OsStr, current: &OsStr) -> Option<OsString> {
    let mut entries: Vec<PathBuf> = env::split_paths(login).filter(|entry| entry.is_absolute()).collect();
    if entries.is_empty() {
        return None;
    }
    for entry in env::split_paths(current) {
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    env::join_paths(entries).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_comes_from_between_the_marks_whatever_the_shell_prints_around_it() {
        let noisy = format!("welcome back\n{MARK}:/opt/homebrew/bin:/usr/bin:{MARK}\nlogout\n");
        assert_eq!(parse(&noisy), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(parse(&format!("{MARK}::{MARK}")), None);
        assert_eq!(parse(&format!("{MARK}{MARK}")), None);
        assert_eq!(parse("no marks at all"), None);
        assert_eq!(parse(MARK), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_added_to_the_stored_path_joins_the_end_and_nothing_else_moves() {
        let current = OsStr::new(r"C:\Windows\System32;C:\Program Files\Git\cmd");
        let same = [OsString::from(r"c:\windows\system32\;C:\Program Files\Git\cmd")];
        assert_eq!(with_new_entries(current, &same), None, "case and a final slash are the same folder");
        let added = [
            OsString::from(r"C:\Windows\System32"),
            OsString::from(r"C:\Users\ana\AppData\Local\agy\bin;relative\dir"),
        ];
        assert_eq!(
            with_new_entries(current, &added),
            Some(OsString::from(r"C:\Windows\System32;C:\Program Files\Git\cmd;C:\Users\ana\AppData\Local\agy\bin"))
        );
        assert_eq!(with_new_entries(current, &[]), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_login_path_comes_first_and_keeps_what_the_app_already_had() {
        let merged = merge(OsStr::new("/opt/homebrew/bin:/usr/bin"), OsStr::new("/usr/bin:/bin:/custom")).unwrap();
        assert_eq!(merged, OsString::from("/opt/homebrew/bin:/usr/bin:/bin:/custom"));
        let relative = merge(OsStr::new("node_modules/.bin:/usr/local/bin"), OsStr::new("/usr/bin")).unwrap();
        assert_eq!(relative, OsString::from("/usr/local/bin:/usr/bin"), "relative entries from rc files are dropped");
        assert_eq!(merge(OsStr::new("relative/only"), OsStr::new("/usr/bin")), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_login_shell_reports_its_path() {
        let path = read(Path::new("/bin/sh"), TIMEOUT).expect("sh prints its PATH");
        assert!(env::split_paths(&path).any(|entry| entry.is_absolute()), "{path:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_that_hangs_or_is_missing_leaves_the_path_alone() {
        use std::os::unix::fs::PermissionsExt;
        let script = env::temp_dir().join(format!("octo-hanging-shell-{}", std::process::id()));
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(read(&script, Duration::from_millis(300)), None);
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        std::fs::remove_file(&script).unwrap();
        assert_eq!(read(Path::new("/nonexistent/shell"), TIMEOUT), None);
    }
}
