#[cfg(windows)]
use std::ffi::c_void;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use orkai::fsutil;
use orkai::paths::Home;
use orkai::util::fnv1a64;

#[cfg(windows)]
const APP_ICON: &[u8] = include_bytes!("../icons/icon.ico");
const MARKER_FILE: &str = "shell-icon";
#[cfg(windows)]
const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
#[cfg(windows)]
const SHCNF_FLUSH: u32 = 0x1000;
const NOTIFY_WAIT: Duration = Duration::from_secs(2);
#[cfg(target_os = "macos")]
const BUNDLE_ICON: &str = "Contents/Resources/icon.icns";
#[cfg(target_os = "macos")]
const BUNDLE_IDENTIFIER: &str = "dev.marujo.octo";
#[cfg(target_os = "macos")]
const DEFAULTS: &str = "/usr/bin/defaults";
#[cfg(target_os = "macos")]
const GETCONF: &str = "/usr/bin/getconf";
#[cfg(target_os = "macos")]
const KILLALL: &str = "/usr/bin/killall";
#[cfg(target_os = "macos")]
const DOCK_PREFERENCES: &str = "com.apple.dock";
#[cfg(target_os = "macos")]
const USER_CACHE_DIR: &str = "DARWIN_USER_CACHE_DIR";
#[cfg(target_os = "macos")]
const DOCK_ICON_CACHE: &str = "com.apple.dock.iconcache";
#[cfg(target_os = "macos")]
const DOCK: &str = "Dock";

#[cfg(windows)]
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHChangeNotify(event: i32, flags: u32, first: *const c_void, second: *const c_void);
}

#[cfg(windows)]
pub fn refresh(home: &Home) {
    refresh_with(&home.root().join(MARKER_FILE), APP_ICON, || {
        wait_briefly(|| unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_FLUSH, std::ptr::null(), std::ptr::null()) })
    });
}

#[cfg(target_os = "macos")]
pub fn refresh(home: &Home) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(bundle) = orkai::update::app_bundle(&exe) else { return };
    if crate::updates::runs_translocated(&exe) {
        return;
    }
    let Ok(icon) = std::fs::read(bundle.join(BUNDLE_ICON)) else { return };
    refresh_with(&home.root().join(MARKER_FILE), &icon, || wait_briefly(redraw_dock_tile));
}

#[cfg(target_os = "macos")]
fn redraw_dock_tile() {
    let Some(preferences) = output_of(DEFAULTS, &["read", DOCK_PREFERENCES]) else { return };
    if !dock_shows_the_app(&preferences) {
        return;
    }
    let Some(cache) = output_of(GETCONF, &[USER_CACHE_DIR]) else { return };
    let _ = std::fs::remove_file(dock_icon_cache(&cache));
    let _ = Command::new(KILLALL).arg(DOCK).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

#[cfg(target_os = "macos")]
fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "macos")]
fn dock_shows_the_app(preferences: &str) -> bool {
    preferences.contains(BUNDLE_IDENTIFIER)
}

#[cfg(target_os = "macos")]
fn dock_icon_cache(cache: &str) -> std::path::PathBuf {
    Path::new(cache.trim()).join(DOCK_ICON_CACHE)
}

fn refresh_with(marker: &Path, icon: &[u8], notify: impl FnOnce()) {
    let current = format!("{:016x}", fnv1a64(icon));
    if std::fs::read_to_string(marker).is_ok_and(|shown| shown == current) {
        return;
    }
    notify();
    let _ = fsutil::write_atomic(marker, current.as_bytes());
}

fn wait_briefly(notify: impl FnOnce() + Send + 'static) {
    let (done, finished) = mpsc::channel();
    let spawned = thread::Builder::new().name("octo-shell-icon".into()).spawn(move || {
        notify();
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

    #[cfg(windows)]
    const PREVIOUS_ICON_MARKER: &str = "43a707abb5957020";

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

    #[cfg(windows)]
    #[test]
    fn the_app_tracks_the_icon_it_is_built_with() {
        assert_eq!(APP_ICON[..4], [0, 0, 1, 0], "an .ico file");
    }

    #[cfg(windows)]
    #[test]
    fn someone_updating_from_the_previous_icon_hears_about_this_one_once() {
        let dir = dir("previous");
        let marker = dir.join(MARKER_FILE);
        std::fs::write(&marker, PREVIOUS_ICON_MARKER).unwrap();
        let told = Cell::new(0);
        refresh_with(&marker, APP_ICON, || told.set(told.get() + 1));
        assert_eq!(told.get(), 1, "the marker of the previous icon must not match this one");
        refresh_with(&marker, APP_ICON, || told.set(told.get() + 1));
        assert_eq!(told.get(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_icons_of_the_app_are_the_ones_rendered_from_the_vector_of_the_site() {
        let rendered = [
            ("site/favicon.svg", include_bytes!("../../site/favicon.svg").as_slice(), 0x9bbe_29df_97ba_36d8_u64),
            ("desktop/icons/icon.ico", include_bytes!("../icons/icon.ico").as_slice(), 0xdeb5_b274_6e9c_b7e3),
            ("desktop/icons/icon.icns", include_bytes!("../icons/icon.icns").as_slice(), 0x36ff_4f48_69b4_b749),
            ("desktop/icons/icon.png", include_bytes!("../icons/icon.png").as_slice(), 0x68de_a4c7_9eac_daa5),
        ];
        for (file, bytes, pinned) in rendered {
            assert_eq!(fnv1a64(bytes), pinned, "{file}: render every size from the grids again and pin them here");
        }
    }

    #[test]
    fn the_icon_a_bundle_brings_later_counts_even_when_the_program_was_updated_first() {
        let dir = dir("bundle");
        let marker = dir.join(MARKER_FILE);
        let told = Cell::new(0);
        refresh_with(&marker, b"old icon inside the old bundle, with a new program", || told.set(told.get() + 1));
        refresh_with(&marker, b"new icon inside the new bundle", || told.set(told.get() + 1));
        assert_eq!(told.get(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_dock_only_restarts_for_someone_who_keeps_the_app_on_it() {
        let pinned = r#"{ "persistent-apps" = ( { "tile-data" = { "bundle-identifier" = "dev.marujo.octo"; }; } ); }"#;
        assert!(dock_shows_the_app(pinned));
        let other = r#"{ "persistent-apps" = ( { "tile-data" = { "bundle-identifier" = "com.apple.Safari"; }; } ); }"#;
        assert!(!dock_shows_the_app(other));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_stale_tile_lives_in_the_icon_cache_of_the_dock_in_the_folder_macos_answers() {
        assert_eq!(
            dock_icon_cache("/var/folders/n6/abc/C/\n"),
            PathBuf::from("/var/folders/n6/abc/C/com.apple.dock.iconcache")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_icon_the_bundle_carries_is_the_one_that_ships() {
        assert_eq!(BUNDLE_ICON, "Contents/Resources/icon.icns", "the release copies it there");
        assert_eq!(&include_bytes!("../icons/icon.icns")[..4], b"icns");
    }
}
