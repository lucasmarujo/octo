use std::io;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;

use crate::termlink;

#[cfg(windows)]
const OPENERS: &[(&str, &[&str])] = &[("rundll32", &["url.dll,FileProtocolHandler"])];

#[cfg(target_os = "macos")]
const OPENERS: &[(&str, &[&str])] = &[("open", &[])];

#[cfg(all(unix, not(target_os = "macos")))]
const OPENERS: &[(&str, &[&str])] = &[("xdg-open", &[])];

#[cfg(not(any(windows, unix)))]
const OPENERS: &[(&str, &[&str])] = &[];

pub fn open(url: String) {
    let _ = thread::Builder::new().name("orkai-browser".into()).spawn(move || open_with(OPENERS, &url));
}

fn open_with(openers: &[(&str, &[&str])], url: &str) -> bool {
    termlink::is_web_link(url)
        && openers
            .iter()
            .filter_map(|(name, args)| which::which(name).ok().map(|program| (program, *args)))
            .any(|(program, args)| run(&program, args, url).is_ok_and(|status| status.success()))
}

fn run(program: &Path, args: &[&str], url: &str) -> io::Result<ExitStatus> {
    Command::new(program).args(args).arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://example.com/docs?a=1&b=2";

    #[test]
    fn the_first_opener_that_works_gets_the_link_and_only_web_links_are_opened() {
        let dir = std::env::temp_dir().join(format!("orkai-browser-{}-{}", std::process::id(), crate::util::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("check.ps1");
        std::fs::write(&script, format!("param($url) if ($url -ceq '{URL}') {{ exit 0 }} exit 1")).unwrap();
        let script = script.to_string_lossy().into_owned();
        let check = format!("test \"$1\" = '{URL}'");
        let (shell, checker) = if cfg!(windows) { ("cmd", "powershell") } else { ("sh", "sh") };
        let (fail, succeed): (&[&str], &[&str]) = if cfg!(windows) {
            (&["/d", "/c", "exit", "1"], &["/d", "/c", "exit", "0"])
        } else {
            (&["-c", "exit 1"], &["-c", "exit 0"])
        };
        let check_args = if cfg!(windows) {
            vec!["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &script]
        } else {
            vec!["-c", &check, "sh"]
        };
        let (failing, succeeding, checking) = ((shell, fail), (shell, succeed), (checker, check_args.as_slice()));
        let missing: (&str, &[&str]) = ("orkai-missing-browser", &[]);
        assert!(!open_with(&[missing, failing], URL));
        assert!(!open_with(&[succeeding], "file:///etc/passwd"), "only web links are opened");
        assert!(!open_with(&[succeeding], "javascript:alert(1)"));
        assert!(!open_with(&[checking], "https://example.com/other"));
        assert!(open_with(&[missing, failing, checking], URL), "the link reaches the opener unchanged");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
