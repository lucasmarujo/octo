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
    if let Some(path) = merge(&login, &current) {
        unsafe {
            env::set_var("PATH", path);
        }
    }
}

fn read(shell: &Path, timeout: Duration) -> Option<OsString> {
    let mut child = Command::new(shell)
        .args(["-ilc", &format!("printf '%s' \"{MARK}$PATH{MARK}\"")])
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
    let path = output.get(start..end)?.trim();
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
        let noisy = format!("welcome back\n{MARK}/opt/homebrew/bin:/usr/bin{MARK}\nlogout\n");
        assert_eq!(parse(&noisy), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(parse(&format!("{MARK}{MARK}")), None);
        assert_eq!(parse("no marks at all"), None);
        assert_eq!(parse(MARK), None);
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
