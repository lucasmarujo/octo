use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

#[cfg(windows)]
const COPIERS: &[(&str, &[&str])] = &[("clip", &[])];

#[cfg(target_os = "macos")]
const COPIERS: &[(&str, &[&str])] = &[("pbcopy", &[])];

#[cfg(all(unix, not(target_os = "macos")))]
const COPIERS: &[(&str, &[&str])] =
    &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["--clipboard", "--input"])];

#[cfg(not(any(windows, unix)))]
const COPIERS: &[(&str, &[&str])] = &[];

pub fn terminal_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text))
}

pub fn copy(text: String) {
    let _ = thread::Builder::new().name("orkai-clipboard".into()).spawn(move || copy_with(COPIERS, &text));
}

fn copy_with(copiers: &[(&str, &[&str])], text: &str) -> bool {
    copiers
        .iter()
        .filter_map(|(name, args)| which::which(name).ok().map(|program| (program, *args)))
        .any(|(program, args)| run(&program, args, text).is_ok_and(|status| status.success()))
}

fn run(program: &Path, args: &[&str], text: &str) -> io::Result<ExitStatus> {
    let mut child =
        Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    let written = child.stdin.take().map_or(Ok(()), |mut stdin| stdin.write_all(text.as_bytes()));
    let status = child.wait()?;
    written.map(|()| status)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "https://quiet-river-sound-lamp.trycloudflare.com/#k=Qm9vdHN0cmFwcGluZyB0aGUgb3JrYWkgcGFpcmluZw";

    #[test]
    fn the_terminal_copies_the_link_through_osc_52() {
        let sequence = terminal_sequence(LINK);
        let encoded = sequence.strip_prefix("\x1b]52;c;").and_then(|rest| rest.strip_suffix('\x07')).unwrap();
        assert_eq!(STANDARD.decode(encoded).unwrap(), LINK.as_bytes());
        let hostile = terminal_sequence("x\x07\x1b]0;pwned\x07");
        assert_eq!(hostile.matches('\x07').count(), 1, "the text never ends the sequence early: {hostile:?}");
        assert!(!hostile[1..].contains('\x1b'));
    }

    #[test]
    fn the_first_clipboard_program_that_works_gets_the_link() {
        let out =
            std::env::temp_dir().join(format!("orkai-clipboard-{}-{}", std::process::id(), crate::util::now_ms()));
        let path = out.to_string_lossy().into_owned();
        let (shell, failing, reading): (&str, Vec<&str>, Vec<&str>) = if cfg!(windows) {
            ("cmd", vec!["/d", "/c", "exit", "1"], vec!["/d", "/c", "more", ">", &path])
        } else {
            ("sh", vec!["-c", "exit 1"], vec!["-c", "cat > \"$1\"", "sh", &path])
        };
        let missing: (&str, &[&str]) = ("orkai-missing-clipboard", &[]);
        assert!(!copy_with(&[missing, (shell, &failing)], LINK), "no program took the link");
        assert!(!out.exists());
        assert!(copy_with(&[missing, (shell, &failing), (shell, &reading)], LINK));
        let copied = std::fs::read_to_string(&out).unwrap();
        let _ = std::fs::remove_file(&out);
        assert_eq!(copied.trim_end(), LINK);
    }
}
