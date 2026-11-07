use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use orkai::fsutil;
use orkai::paths::Home;

const APP_ID: &str = "dev.marujo.octo";
const APP_NAME: &str = "Octo";
const REGISTRY_FOLDER: &str = r"Software\Classes\AppUserModelId";
const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
const REG_SZ: u32 = 1;
const DISPLAY_NAME: &str = "DisplayName";
const ICON_URI: &str = "IconUri";
const ICON_FILE: &str = "notification-icon.png";
const ICON: &[u8] = include_bytes!("../icons/icon.png");

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegSetKeyValueW(
        key: isize,
        sub_key: *const u16,
        value_name: *const u16,
        kind: u32,
        data: *const c_void,
        size: u32,
    ) -> i32;
}

fn write_icon(folder: &Path) -> Option<PathBuf> {
    let path = folder.join(ICON_FILE);
    let current = std::fs::read(&path).is_ok_and(|saved| saved == ICON);
    (current || fsutil::write_atomic(&path, ICON).is_ok()).then_some(path)
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

fn set_value(name: &str, value: &OsStr) {
    let sub_key = wide(format!(r"{REGISTRY_FOLDER}\{APP_ID}").as_ref());
    let value_name = wide(name.as_ref());
    let data = wide(value);
    let Ok(size) = u32::try_from(data.len() * size_of::<u16>()) else { return };
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, sub_key.as_ptr(), value_name.as_ptr(), REG_SZ, data.as_ptr().cast(), size);
    }
}

pub fn register(home: &Home) {
    set_value(DISPLAY_NAME, APP_NAME.as_ref());
    if let Some(icon) = write_icon(home.root()) {
        set_value(ICON_URI, icon.as_os_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "octo-toast-app-{name}-{}-{}",
            std::process::id(),
            orkai::util::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_icon_of_the_toast_is_written_once_and_replaced_when_it_changes() {
        let dir = dir("icon");
        let path = write_icon(&dir).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), ICON);
        std::fs::write(&path, b"the icon of an older version").unwrap();
        assert_eq!(write_icon(&dir), Some(path.clone()));
        assert_eq!(std::fs::read(&path).unwrap(), ICON);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn windows_only_shows_toasts_of_the_id_the_notification_plugin_uses() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], APP_ID, "the plugin sends toasts under the identifier of the app");
        assert_eq!(wide("Oi".as_ref()), [79, 105, 0], "the registry takes UTF-16 ending in a zero");
    }
}
