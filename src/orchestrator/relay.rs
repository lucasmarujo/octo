use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::orchestrator::OrchestratorView;
use crate::paths::{Home, create_private_dir};
use crate::pwa::actions::notice;
use crate::pwa::hub::{Event, Hub};
use crate::pwa::protocol::{PhoneMsg, Tone};
use crate::util::now_ms;

pub const MAX_COMMANDS_PER_TICK: usize = 16;
pub const COMMAND_TTL_MS: u64 = 60_000;
pub const MAX_VIEW_BYTES: u64 = 8 * 1024 * 1024;
pub const OFFLINE: &str = "Open Octo on the computer to use the orchestrator";
const VIEW_FILE: &str = "view.json";
const INBOX_DIR: &str = "inbox";
const PRESENCE_FILE: &str = "desktop.lock";
const COMMAND_SUFFIX: &str = ".json";
#[cfg(windows)]
const SHARING_VIOLATION: i32 = 32;

static COMMAND_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn view_file(home: &Home) -> PathBuf {
    home.orchestrator_dir().join(VIEW_FILE)
}

pub fn inbox_dir(home: &Home) -> PathBuf {
    home.orchestrator_dir().join(INBOX_DIR)
}

pub fn presence_file(home: &Home) -> PathBuf {
    home.orchestrator_dir().join(PRESENCE_FILE)
}

pub struct Presence {
    _held: File,
}

#[cfg(windows)]
fn lock_exclusively(options: &mut OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt;
    options.share_mode(0);
}

#[cfg(unix)]
fn lock_exclusively(_options: &mut OpenOptions) {}

#[cfg(windows)]
fn claim(_file: &File) -> bool {
    true
}

#[cfg(unix)]
fn claim(file: &File) -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

#[cfg(windows)]
fn held_by_another(opened: io::Result<File>) -> bool {
    opened.is_err_and(|err| err.raw_os_error() == Some(SHARING_VIOLATION))
}

#[cfg(unix)]
fn held_by_another(opened: io::Result<File>) -> bool {
    use std::os::unix::io::AsRawFd;
    opened.is_ok_and(|file| unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) != 0 })
}

pub fn hold(home: &Home) -> io::Result<Presence> {
    create_private_dir(&home.orchestrator_dir())?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    lock_exclusively(&mut options);
    let file = options.open(presence_file(home))?;
    if !claim(&file) {
        return Err(io::Error::new(io::ErrorKind::WouldBlock, "another Octo already orchestrates here"));
    }
    Ok(Presence { _held: file })
}

pub fn desktop_present(home: &Home) -> bool {
    held_by_another(File::open(presence_file(home)))
}

pub fn write_view(home: &Home, view: &OrchestratorView) -> io::Result<()> {
    create_private_dir(&home.orchestrator_dir())?;
    let data = serde_json::to_vec(view).map_err(io::Error::other)?;
    fsutil::write_atomic(&view_file(home), &data)
}

fn read_view(home: &Home) -> Option<OrchestratorView> {
    let mut data = Vec::new();
    File::open(view_file(home)).ok()?.take(MAX_VIEW_BYTES).read_to_end(&mut data).ok()?;
    serde_json::from_slice(&data).ok()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub at_ms: u64,
    pub message: PhoneMsg,
}

pub fn send_command(home: &Home, message: &PhoneMsg, now: u64) -> io::Result<()> {
    let dir = inbox_dir(home);
    create_private_dir(&dir)?;
    let counter = COMMAND_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("{now:016}-{:010}-{counter:010}{COMMAND_SUFFIX}", std::process::id());
    fsutil::write_json_atomic(&dir.join(name), &Command { at_ms: now, message: message.clone() })
}

pub fn forward(hub: &Hub, command: &PhoneMsg, reply: &Sender<Event>) {
    if !hub.followed().online {
        return notice(reply, Tone::Warn, OFFLINE);
    }
    if let Err(err) = send_command(hub.store().home(), command, now_ms()) {
        notice(reply, Tone::Error, format!("could not reach Octo on the computer: {err}"));
    }
}

fn is_command(name: &str) -> bool {
    name.ends_with(COMMAND_SUFFIX) && !name.starts_with('.')
}

fn abandoned(entry: &fs::DirEntry, now: u64) -> bool {
    let written = entry.metadata().ok().and_then(|meta| meta.modified().ok());
    let written = written.and_then(|at| at.duration_since(UNIX_EPOCH).ok());
    written.is_some_and(|at| now.saturating_sub(u64::try_from(at.as_millis()).unwrap_or(u64::MAX)) > COMMAND_TTL_MS)
}

pub fn has_commands(home: &Home) -> bool {
    let Ok(entries) = fs::read_dir(inbox_dir(home)) else { return false };
    entries.flatten().any(|entry| entry.file_name().to_str().is_some_and(is_command))
}

pub fn take_commands(home: &Home, now: u64) -> Vec<PhoneMsg> {
    let dir = inbox_dir(home);
    let Ok(entries) = fs::read_dir(&dir) else { return Vec::new() };
    let mut names: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        match entry.file_name().to_str() {
            Some(name) if is_command(name) => names.push(name.to_string()),
            _ if abandoned(&entry, now) => {
                let _ = fs::remove_file(entry.path());
            }
            _ => {}
        }
    }
    names.sort_unstable();
    names.truncate(MAX_COMMANDS_PER_TICK);
    let mut messages = Vec::new();
    for name in names {
        let path = dir.join(&name);
        let command = fsutil::read_json::<Command>(&path).ok();
        let _ = fs::remove_file(&path);
        if let Some(command) = command.filter(|command| now.saturating_sub(command.at_ms) <= COMMAND_TTL_MS) {
            messages.push(command.message);
        }
    }
    messages
}

type Stamp = (SystemTime, u64);

fn stamp_of(home: &Home) -> Option<Stamp> {
    let meta = fs::metadata(view_file(home)).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

#[derive(Default)]
pub struct Follower {
    stamp: Option<Stamp>,
    view: Option<OrchestratorView>,
    online: Option<bool>,
}

impl Follower {
    pub fn refresh(&mut self, home: &Home, probe: bool) -> bool {
        let mut changed = false;
        if probe || self.online.is_none() {
            let online = Some(desktop_present(home));
            changed = online != self.online;
            self.online = online;
        }
        let stamp = stamp_of(home);
        if stamp != self.stamp {
            self.stamp = stamp;
            self.view = read_view(home).or(self.view.take().filter(|_| stamp.is_some()));
            changed = true;
        }
        changed
    }

    pub fn online(&self) -> bool {
        self.online == Some(true)
    }

    pub fn shown(&self) -> OrchestratorView {
        OrchestratorView { online: self.online(), ..self.view.clone().unwrap_or_default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{Mode, SettingsChange};

    fn temp_home(tag: &str) -> (PathBuf, Home) {
        let dir = std::env::temp_dir().join(format!("orkai-octo-relay-{tag}-{}-{}", std::process::id(), now_ms()));
        (dir.clone(), Home::at(dir))
    }

    #[test]
    fn the_desktop_is_present_only_while_it_holds_its_lock() {
        let (dir, home) = temp_home("presence");
        assert!(!desktop_present(&home), "nothing was ever opened here");
        assert!(!dir.exists(), "asking writes nothing");
        let held = hold(&home).unwrap();
        assert!(desktop_present(&home));
        assert!(desktop_present(&home), "asking twice does not take the lock away");
        assert!(hold(&home).is_err(), "only one Octo holds it");
        drop(held);
        assert!(!desktop_present(&home), "a desktop that is gone is gone, whatever its file says");
        assert!(presence_file(&home).exists());
        let again = hold(&home).unwrap();
        assert!(desktop_present(&home));
        drop(again);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn commands_are_read_once_oldest_first_in_bounded_batches_and_stale_ones_are_dropped() {
        let (dir, home) = temp_home("inbox");
        assert!(!has_commands(&home));
        assert!(take_commands(&home, 1).is_empty());
        let chat = |text: &str| PhoneMsg::OrchestratorChat { text: text.into() };
        let now = 500_000;
        send_command(&home, &chat("too old"), now - COMMAND_TTL_MS - 1).unwrap();
        send_command(&home, &chat("first"), now - COMMAND_TTL_MS).unwrap();
        send_command(&home, &chat("third"), now).unwrap();
        send_command(&home, &chat("second"), now - 10).unwrap();
        send_command(&home, &PhoneMsg::OrchestratorClearChat, now).unwrap();
        fs::write(inbox_dir(&home).join(".half.json.1.0.tmp"), b"{").unwrap();
        fs::write(inbox_dir(&home).join("0000000000000001-0000000001-0000000001.json"), b"broken").unwrap();
        assert!(has_commands(&home));
        assert_eq!(
            take_commands(&home, now),
            [chat("first"), chat("second"), chat("third"), PhoneMsg::OrchestratorClearChat],
            "in the order they were sent, without the one that waited more than a minute"
        );
        assert!(take_commands(&home, now).is_empty(), "each command is read once");
        let left: Vec<_> = fs::read_dir(inbox_dir(&home)).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert_eq!(left.len(), 1, "only the file still being written remains: {left:?}");

        for index in 0..MAX_COMMANDS_PER_TICK + 3 {
            send_command(&home, &chat(&index.to_string()), now + u64::try_from(index).unwrap()).unwrap();
        }
        assert_eq!(take_commands(&home, now).len(), MAX_COMMANDS_PER_TICK);
        assert_eq!(take_commands(&home, now).len(), 3);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_left_half_written_is_not_a_command_and_is_swept_once_it_is_old() {
        let (dir, home) = temp_home("leftover");
        fs::create_dir_all(inbox_dir(&home)).unwrap();
        let leftover = inbox_dir(&home).join(".0000000000000001-1.json.9.0.tmp");
        let stray = inbox_dir(&home).join("notes.txt");
        fs::write(&leftover, b"{").unwrap();
        fs::write(&stray, b"x").unwrap();
        assert!(!has_commands(&home), "what is not a command wakes nobody up");
        let now = now_ms();
        assert!(take_commands(&home, now).is_empty());
        assert!(leftover.exists() && stray.exists(), "a file that may still be being written is left alone");
        let long_ago = SystemTime::now() - std::time::Duration::from_millis(COMMAND_TTL_MS + 5_000);
        File::options().write(true).open(&leftover).unwrap().set_modified(long_ago).unwrap();
        send_command(&home, &PhoneMsg::OrchestratorClearChat, now).unwrap();
        assert!(has_commands(&home));
        assert_eq!(take_commands(&home, now), [PhoneMsg::OrchestratorClearChat]);
        assert!(!leftover.exists(), "one abandoned for more than a minute is deleted");
        assert!(stray.exists());
        assert!(!has_commands(&home));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_command_keeps_the_exact_message_of_the_phone() {
        let (dir, home) = temp_home("command");
        let set = PhoneMsg::OrchestratorSet(SettingsChange {
            enabled: Some(true),
            primary: Some(None),
            ..SettingsChange::default()
        });
        send_command(&home, &set, 7).unwrap();
        let name = fs::read_dir(inbox_dir(&home)).unwrap().flatten().next().unwrap().file_name();
        let text = fs::read_to_string(inbox_dir(&home).join(&name)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["at_ms"], 7);
        assert_eq!(
            value["message"],
            serde_json::json!({ "type": "orchestratorSet", "enabled": true, "primary": null })
        );
        assert_eq!(take_commands(&home, 8), [set]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_bridge_follows_the_view_file_and_says_whether_the_desktop_is_there() {
        let (dir, home) = temp_home("follower");
        let mut follower = Follower::default();
        assert!(follower.refresh(&home, false), "the first look always tells the phone something");
        assert_eq!(follower.shown(), OrchestratorView::default(), "with no desktop it is off and offline");
        assert!(!follower.shown().online && !follower.online());
        assert!(!follower.refresh(&home, true), "nothing changed");

        let held = hold(&home).unwrap();
        assert!(!follower.refresh(&home, false), "the desktop is looked for only when asked");
        assert!(follower.refresh(&home, true));
        assert!(follower.shown().online);

        let view =
            OrchestratorView { enabled: true, mode: Mode::Moderate, online: true, ..OrchestratorView::default() };
        write_view(&home, &view).unwrap();
        assert!(follower.refresh(&home, false), "a new view is seen on the next look");
        assert_eq!(follower.shown(), view);
        assert!(!follower.refresh(&home, false), "and told once");

        drop(held);
        assert!(follower.refresh(&home, true));
        let offline = follower.shown();
        assert!(!offline.online, "the view of a desktop that closed is shown as offline");
        assert_eq!((offline.enabled, offline.mode), (true, Mode::Moderate), "with what was last known");

        fs::write(view_file(&home), b"{ half").unwrap();
        assert!(follower.refresh(&home, false));
        assert_eq!(follower.shown().mode, Mode::Moderate, "a view that cannot be read keeps the last one");
        fs::remove_file(view_file(&home)).unwrap();
        assert!(follower.refresh(&home, false));
        assert_eq!(follower.shown(), OrchestratorView::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
