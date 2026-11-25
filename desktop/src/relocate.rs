use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use orkai::hooks::parse_version;
use orkai::paths::user_home;
use orkai::update::{app_bundle, copy_app, release_from_quarantine};

use crate::shell_icon::{BUNDLE_IDENTIFIER, DEFAULTS, output_of};
use crate::updates::runs_translocated;

const APPLICATIONS: &str = "/Applications";
const USER_APPLICATIONS: &str = "Applications";
const INFO_PLIST: &str = "Contents/Info.plist";
const IDENTIFIER_KEY: &str = "CFBundleIdentifier";
const VERSION_KEY: &str = "CFBundleShortVersionString";
const SHELL: &str = "/bin/sh";
const OPEN_ONCE_GONE: &str = r#"while /bin/kill -0 "$1" 2>/dev/null; do /bin/sleep 0.1; done; exec /usr/bin/open "$2""#;

fn plist_value(bundle: &Path, key: &str) -> Option<String> {
    let plist = bundle.join(INFO_PLIST);
    output_of(DEFAULTS, &["read", plist.to_str()?, key]).map(|value| value.trim().to_string())
}

fn is_as_new_as_this_one(installed: &Path) -> bool {
    let running = parse_version(env!("CARGO_PKG_VERSION"));
    let there = plist_value(installed, VERSION_KEY).and_then(|version| parse_version(&version));
    there.zip(running).is_some_and(|(there, running)| there >= running)
}

fn copy_into(bundle: &Path, folder: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(folder)
        .and_then(|()| copy_app(bundle, folder))
        .inspect_err(|err| eprintln!("octo: could not install itself in {}: {err}", folder.display()))
        .ok()
}

fn place_in(bundle: &Path, folder: &Path) -> Option<PathBuf> {
    let installed = folder.join(bundle.file_name()?);
    if !installed.exists() {
        return copy_into(bundle, folder);
    }
    if plist_value(&installed, IDENTIFIER_KEY)? != BUNDLE_IDENTIFIER {
        return None;
    }
    if is_as_new_as_this_one(&installed) {
        return release_from_quarantine(&installed).then_some(installed);
    }
    copy_into(bundle, folder)
}

pub fn install_in_first(exe: &Path, folders: &[PathBuf]) -> Option<PathBuf> {
    let bundle = app_bundle(exe)?;
    folders.iter().find_map(|folder| place_in(bundle, folder))
}

fn folders() -> Vec<PathBuf> {
    let of_the_user = user_home().map(|home| home.join(USER_APPLICATIONS));
    std::iter::once(PathBuf::from(APPLICATIONS)).chain(of_the_user).collect()
}

fn open_once_gone(installed: &Path) -> bool {
    Command::new(SHELL)
        .arg("-c")
        .arg(OPEN_ONCE_GONE)
        .arg(SHELL)
        .arg(std::process::id().to_string())
        .arg(installed)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

pub fn leave_read_only_copy() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    if !runs_translocated(&exe) {
        return false;
    }
    install_in_first(&exe, &folders()).is_some_and(|installed| open_once_gone(&installed))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use orkai::util::now_ms;

    use super::*;

    const OLDER: &str = "0.0.1";
    const NEWER: &str = "999.0.0";
    const EXECUTABLE: &str = "Contents/MacOS/octo";

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("octo-relocate-{tag}-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn app(folder: &Path, identifier: &str, version: &str, binary: &[u8]) -> PathBuf {
        let bundle = folder.join("Octo.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join(EXECUTABLE), binary).unwrap();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>octo</string>
  <key>CFBundleIdentifier</key><string>{identifier}</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
</dict>
</plist>
"#
        );
        fs::write(bundle.join(INFO_PLIST), plist).unwrap();
        bundle
    }

    fn downloaded(folder: &Path) -> PathBuf {
        app(folder, BUNDLE_IDENTIFIER, env!("CARGO_PKG_VERSION"), b"this one").join(EXECUTABLE)
    }

    fn binary_in(bundle: &Path) -> Vec<u8> {
        fs::read(bundle.join(EXECUTABLE)).unwrap()
    }

    #[test]
    fn the_app_installs_itself_in_the_first_folder_that_takes_it() {
        let dir = temp_dir("first");
        let exe = downloaded(&dir.join("Downloads"));
        let applications = dir.join("Applications");
        fs::create_dir_all(&applications).unwrap();
        let installed = install_in_first(&exe, &[applications.clone(), dir.join("Mine")]).unwrap();
        assert_eq!(installed, applications.join("Octo.app"));
        assert_eq!(binary_in(&installed), b"this one");
        assert!(!dir.join("Mine").exists(), "the second folder is only for when the first refuses");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_older_octo_already_installed_is_replaced_by_this_one() {
        let dir = temp_dir("older");
        let exe = downloaded(&dir.join("Downloads"));
        let applications = dir.join("Applications");
        app(&applications, BUNDLE_IDENTIFIER, OLDER, b"older");
        let installed = install_in_first(&exe, std::slice::from_ref(&applications)).unwrap();
        assert_eq!(installed, applications.join("Octo.app"));
        assert_eq!(binary_in(&installed), b"this one");
        assert_eq!(plist_value(&installed, VERSION_KEY).as_deref(), Some(env!("CARGO_PKG_VERSION")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_octo_already_installed_that_is_as_new_is_opened_as_it_is() {
        let dir = temp_dir("newer");
        let exe = downloaded(&dir.join("Downloads"));
        for (version, binary) in [(NEWER, b"newer".as_slice()), (env!("CARGO_PKG_VERSION"), b"same".as_slice())] {
            let applications = dir.join(version);
            app(&applications, BUNDLE_IDENTIFIER, version, binary);
            let installed = install_in_first(&exe, std::slice::from_ref(&applications)).unwrap();
            assert_eq!(installed, applications.join("Octo.app"));
            assert_eq!(binary_in(&installed), binary, "an update already made there is never undone");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn another_app_with_the_same_name_is_left_alone_and_the_next_folder_is_used() {
        let dir = temp_dir("foreign");
        let exe = downloaded(&dir.join("Downloads"));
        let applications = dir.join("Applications");
        let foreign = app(&applications, "com.example.octo", OLDER, b"not ours");
        let mine = dir.join("Home").join("Applications");
        let installed = install_in_first(&exe, &[applications, mine.clone()]).unwrap();
        assert_eq!(installed, mine.join("Octo.app"));
        assert_eq!(binary_in(&installed), b"this one");
        assert_eq!(binary_in(&foreign), b"not ours");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nothing_is_installed_when_no_folder_takes_the_app_or_it_is_not_an_app() {
        let dir = temp_dir("nowhere");
        let exe = downloaded(&dir.join("Downloads"));
        let blocked = dir.join("blocked");
        fs::write(&blocked, b"a file where a folder was expected").unwrap();
        assert_eq!(install_in_first(&exe, &[blocked.join("Applications")]), None);
        assert_eq!(install_in_first(&exe, &[]), None);
        let loose = dir.join("octo");
        fs::write(&loose, b"binary").unwrap();
        assert_eq!(install_in_first(&loose, &[dir.join("Applications")]), None);
        assert!(!dir.join("Applications").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
