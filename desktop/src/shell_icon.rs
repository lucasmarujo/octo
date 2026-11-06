use std::ffi::c_void;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use orkai::fsutil;
use orkai::paths::Home;
use orkai::util::fnv1a64;

const APP_ICON: &[u8] = include_bytes!("../icons/icon.ico");
const MARKER_FILE: &str = "shell-icon";
const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
const SHCNF_FLUSH: u32 = 0x1000;
const NOTIFY_WAIT: Duration = Duration::from_secs(2);

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHChangeNotify(event: i32, flags: u32, first: *const c_void, second: *const c_void);
}

pub fn refresh(home: &Home) {
    refresh_with(&home.root().join(MARKER_FILE), APP_ICON, notify_shell);
}

fn refresh_with(marker: &Path, icon: &[u8], notify: impl FnOnce()) {
    let current = format!("{:016x}", fnv1a64(icon));
    if std::fs::read_to_string(marker).is_ok_and(|shown| shown == current) {
        return;
    }
    notify();
    let _ = fsutil::write_atomic(marker, current.as_bytes());
}

fn notify_shell() {
    let (done, finished) = mpsc::channel();
    let spawned = thread::Builder::new().name("octo-shell-icon".into()).spawn(move || {
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_FLUSH, std::ptr::null(), std::ptr::null()) };
        let _ = done.send(());
    });
    if spawned.is_ok() {
        let _ = finished.recv_timeout(NOTIFY_WAIT);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;

    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "octo-shell-icon-{name}-{}-{}",
            std::process::id(),
            orkai::util::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_shell_hears_about_an_icon_once_and_again_only_when_it_changes() {
        let dir = dir("once");
        let marker = dir.join(MARKER_FILE);
        let told = Cell::new(0);
        let tell = || told.set(told.get() + 1);

        refresh_with(&marker, b"kai", tell);
        assert_eq!(told.get(), 1, "the first launch of this icon");
        refresh_with(&marker, b"kai", tell);
        assert_eq!(told.get(), 1, "the desktop does not flicker at every launch");
        refresh_with(&marker, b"eye", tell);
        assert_eq!(told.get(), 2, "a new icon in the same path");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_marker_that_cannot_be_written_only_means_telling_the_shell_again() {
        let dir = dir("unwritable");
        let marker = dir.join("missing").join(MARKER_FILE);
        let told = Cell::new(0);
        refresh_with(&marker, b"eye", || told.set(told.get() + 1));
        refresh_with(&marker, b"eye", || told.set(told.get() + 1));
        assert_eq!(told.get(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_app_tracks_the_icon_it_is_built_with() {
        assert_eq!(APP_ICON[..4], [0, 0, 1, 0], "an .ico file");
    }
}
