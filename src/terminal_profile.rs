use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::fsutil::write_if_changed;
use crate::paths::Home;
use crate::update::BUILD_COMMIT;
use crate::util::fnv1a64;

pub const DISABLE_ENV: &str = "ORKAI_NO_TERMINAL_PROFILE";
pub const NAME: &str = "orkai";
pub const ICON_ICO: &[u8] = include_bytes!("../assets/orkai.ico");
pub const ICON_PNG: &[u8] = include_bytes!("../assets/orkai.png");
pub const ICON_URL: &str = "https://orkai.marujo.dev/orkai.ico";
const ICON_ICO_NAME: &str = "orkai.ico";
const FRAGMENT_FILE: &str = "orkai.json";
const ITERM_FILE: &str = "orkai.json";
const DESKTOP_FILE: &str = "orkai.desktop";
const ITERM_PROGRAM: &str = "iTerm.app";
const ITERM_CUSTOM_ICON: u8 = 2;
const ITERM_GUID_PREFIX: &str = "orkai-";
const ITERM_RELOAD: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    WindowsTerminal,
    Iterm { parent: String },
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    pub to: String,
    pub back: String,
    pub delay: Duration,
}

pub fn detect(os: &str, var: impl Fn(&str) -> Option<String>) -> Option<Target> {
    let set = |key: &str| var(key).filter(|value| !value.is_empty());
    if set(DISABLE_ENV).is_some() {
        return None;
    }
    match os {
        "windows" => set("WT_SESSION").map(|_| Target::WindowsTerminal),
        "macos" if set("TERM_PROGRAM").as_deref() == Some(ITERM_PROGRAM) => set("ITERM_PROFILE")
            .filter(|parent| !parent.chars().any(char::is_control))
            .map(|parent| Target::Iterm { parent }),
        "linux" => set("DISPLAY").or_else(|| set("WAYLAND_DISPLAY")).map(|_| Target::Desktop),
        _ => None,
    }
}

pub fn install(home: &Home, exe: &Path) -> Option<Switch> {
    BUILD_COMMIT?;
    match detect(std::env::consts::OS, |key| std::env::var(key).ok())? {
        Target::WindowsTerminal => {
            let dir = dirs::data_local_dir()?.join("Microsoft").join("Windows Terminal").join("Fragments").join(NAME);
            let _ = install_windows_terminal(&dir, exe);
            None
        }
        Target::Iterm { parent } => {
            let dir =
                dirs::home_dir()?.join("Library").join("Application Support").join("iTerm2").join("DynamicProfiles");
            install_iterm(&dir, &home.icon_file(), &parent).ok()
        }
        Target::Desktop => {
            let _ = install_desktop(&dirs::data_dir()?.join("applications"), exe, &home.icon_file());
            None
        }
    }
}

pub fn fragment(exe: &Path) -> String {
    let profile = json!({
        "profiles": [{
            "name": NAME,
            "commandline": format!("\"{}\"", exe.display()),
            "icon": ICON_URL,
        }]
    });
    serde_json::to_string_pretty(&profile).unwrap_or_default()
}

pub fn install_windows_terminal(dir: &Path, exe: &Path) -> io::Result<bool> {
    let icon = write_if_changed(&dir.join(ICON_ICO_NAME), ICON_ICO)?;
    let profile = write_if_changed(&dir.join(FRAGMENT_FILE), fragment(exe).as_bytes())?;
    Ok(icon || profile)
}

pub fn iterm_profile_name(parent: &str) -> String {
    format!("{NAME} · {parent}")
}

fn iterm_guid(parent: &str) -> String {
    format!("{ITERM_GUID_PREFIX}{:016x}", fnv1a64(parent.as_bytes()))
}

pub fn iterm_profiles(existing: Option<&[u8]>, parent: &str, icon: &Path) -> String {
    let guid = iterm_guid(parent);
    let mut profiles: Vec<Value> = existing
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .and_then(|value| value.get("Profiles").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|profile| {
            profile.get("Guid").and_then(Value::as_str).is_some_and(|g| g.starts_with(ITERM_GUID_PREFIX) && g != guid)
        })
        .collect();
    profiles.push(json!({
        "Name": iterm_profile_name(parent),
        "Guid": guid,
        "Dynamic Profile Parent Name": parent,
        "Icon": ITERM_CUSTOM_ICON,
        "Custom Icon Path": icon.display().to_string(),
    }));
    profiles.sort_by(|a, b| a["Guid"].as_str().cmp(&b["Guid"].as_str()));
    serde_json::to_string_pretty(&json!({ "Profiles": profiles })).unwrap_or_default()
}

pub fn install_iterm(dir: &Path, icon: &Path, parent: &str) -> io::Result<Switch> {
    write_if_changed(icon, ICON_PNG)?;
    let file = dir.join(ITERM_FILE);
    let existing = fs::read(&file).ok();
    let changed = write_if_changed(&file, iterm_profiles(existing.as_deref(), parent, icon).as_bytes())?;
    Ok(Switch {
        to: iterm_profile_name(parent),
        back: parent.to_string(),
        delay: if changed { ITERM_RELOAD } else { Duration::ZERO },
    })
}

pub fn set_profile_sequence(name: &str) -> String {
    format!("\x1b]1337;SetProfile={name}\x07")
}

fn desktop_quote(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for c in path.display().to_string().chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

pub fn desktop_entry(exe: &Path, icon: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={NAME}\nComment={}\nExec={}\nIcon={}\nTerminal=true\nCategories=Development;ConsoleOnly;\nKeywords=ai;agents;claude;codex;terminal;\n",
        env!("CARGO_PKG_DESCRIPTION"),
        desktop_quote(exe),
        icon.display()
    )
}

pub fn install_desktop(dir: &Path, exe: &Path, icon: &Path) -> io::Result<bool> {
    let icon_changed = write_if_changed(icon, ICON_PNG)?;
    let entry_changed = write_if_changed(&dir.join(DESKTOP_FILE), desktop_entry(exe, icon).as_bytes())?;
    Ok(icon_changed || entry_changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |key| map.get(key).cloned()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orkai-terminal-profile-{tag}-{}-{}",
            std::process::id(),
            crate::util::now_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_the_terminal_that_can_show_the_icon() {
        assert_eq!(detect("windows", env(&[("WT_SESSION", "abc")])), Some(Target::WindowsTerminal));
        assert_eq!(detect("windows", env(&[])), None);
        assert_eq!(
            detect("macos", env(&[("TERM_PROGRAM", "iTerm.app"), ("ITERM_PROFILE", "Default")])),
            Some(Target::Iterm { parent: "Default".into() })
        );
        assert_eq!(detect("macos", env(&[("TERM_PROGRAM", "Apple_Terminal"), ("ITERM_PROFILE", "Default")])), None);
        assert_eq!(detect("macos", env(&[("TERM_PROGRAM", "tmux"), ("ITERM_PROFILE", "Default")])), None);
        assert_eq!(detect("macos", env(&[("TERM_PROGRAM", "iTerm.app"), ("ITERM_PROFILE", "a\x07b")])), None);
        assert_eq!(detect("linux", env(&[("DISPLAY", ":0")])), Some(Target::Desktop));
        assert_eq!(detect("linux", env(&[("WAYLAND_DISPLAY", "wayland-0")])), Some(Target::Desktop));
        assert_eq!(detect("linux", env(&[("SSH_CONNECTION", "1.2.3.4")])), None);
        assert_eq!(detect("freebsd", env(&[("DISPLAY", ":0")])), None);
    }

    #[test]
    fn the_opt_out_disables_every_terminal() {
        assert_eq!(detect("windows", env(&[("WT_SESSION", "abc"), (DISABLE_ENV, "1")])), None);
        assert_eq!(detect("linux", env(&[("DISPLAY", ":0"), (DISABLE_ENV, "1")])), None);
        assert_eq!(
            detect("macos", env(&[("TERM_PROGRAM", "iTerm.app"), ("ITERM_PROFILE", "Default"), (DISABLE_ENV, "1")])),
            None
        );
        assert_eq!(detect("windows", env(&[("WT_SESSION", "abc"), (DISABLE_ENV, "")])), Some(Target::WindowsTerminal));
    }

    #[test]
    fn the_windows_terminal_fragment_launches_orkai_with_its_icon() {
        let exe = Path::new(r"C:\Users\Jo Ann\AppData\Local\Programs\orkai\orkai.exe");
        let value: Value = serde_json::from_str(&fragment(exe)).unwrap();
        let profile = &value["profiles"][0];
        assert_eq!(profile["name"], NAME);
        assert_eq!(profile["commandline"], format!("\"{}\"", exe.display()));
        assert_eq!(profile["icon"], ICON_URL);
        assert!(ICON_URL.ends_with(&format!("/{ICON_ICO_NAME}")), "Windows Terminal 1.24+ reads the icon by file name");
    }

    #[test]
    fn the_windows_terminal_files_are_written_once() {
        let dir = temp_dir("wt");
        let exe = Path::new(r"C:\orkai\orkai.exe");
        assert!(install_windows_terminal(&dir, exe).unwrap());
        assert_eq!(fs::read(dir.join(ICON_ICO_NAME)).unwrap(), ICON_ICO);
        assert_eq!(fs::read_to_string(dir.join(FRAGMENT_FILE)).unwrap(), fragment(exe));
        assert!(!install_windows_terminal(&dir, exe).unwrap());
        assert!(install_windows_terminal(&dir, Path::new(r"D:\tools\orkai.exe")).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn iterm_profiles_inherit_each_parent_and_only_change_the_icon() {
        let icon = Path::new("/Users/me/Library/Application Support/orkai/orkai.png");
        let first = iterm_profiles(None, "Default", icon);
        let both = iterm_profiles(Some(first.as_bytes()), "Work", icon);
        let again = iterm_profiles(Some(both.as_bytes()), "Default", icon);
        assert_eq!(both, again);
        let value: Value = serde_json::from_str(&both).unwrap();
        let profiles = value["Profiles"].as_array().unwrap();
        assert_eq!(profiles.len(), 2);
        let default = profiles.iter().find(|p| p["Dynamic Profile Parent Name"] == "Default").unwrap();
        assert_eq!(default["Name"], "orkai · Default");
        assert_eq!(default["Icon"], ITERM_CUSTOM_ICON);
        assert_eq!(default["Custom Icon Path"], icon.display().to_string());
        assert!(default["Guid"].as_str().unwrap().starts_with(ITERM_GUID_PREFIX));
        assert_ne!(profiles[0]["Guid"], profiles[1]["Guid"]);
        let foreign = r#"{"Profiles":[{"Name":"mine","Guid":"user-1"}]}"#;
        let replaced: Value = serde_json::from_str(&iterm_profiles(Some(foreign.as_bytes()), "Default", icon)).unwrap();
        assert_eq!(replaced["Profiles"].as_array().unwrap().len(), 1);
        let broken: Value = serde_json::from_str(&iterm_profiles(Some(b"{not json"), "Default", icon)).unwrap();
        assert_eq!(broken["Profiles"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn iterm_switch_waits_only_when_the_profile_is_new() {
        let dir = temp_dir("iterm");
        let icon = dir.join("home").join("orkai.png");
        let profiles = dir.join("DynamicProfiles");
        let first = install_iterm(&profiles, &icon, "Default").unwrap();
        assert_eq!(first, Switch { to: "orkai · Default".into(), back: "Default".into(), delay: ITERM_RELOAD });
        assert_eq!(fs::read(&icon).unwrap(), ICON_PNG);
        assert_eq!(install_iterm(&profiles, &icon, "Default").unwrap().delay, Duration::ZERO);
        assert_eq!(set_profile_sequence("orkai · Default"), "\x1b]1337;SetProfile=orkai · Default\x07");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_desktop_entry_quotes_the_launch_path() {
        let entry = desktop_entry(
            Path::new("/home/jo ann/bin/orkai$1%"),
            Path::new("/home/jo ann/.local/share/orkai/orkai.png"),
        );
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("\nExec=\"/home/jo ann/bin/orkai\\$1%%\"\n"), "{entry}");
        assert!(entry.contains("\nIcon=/home/jo ann/.local/share/orkai/orkai.png\n"));
        assert!(entry.contains("\nTerminal=true\n"));
        assert!(entry.contains(&format!("\nName={NAME}\n")));
    }

    #[test]
    fn the_desktop_files_are_written_once() {
        let dir = temp_dir("desktop");
        let icon = dir.join("orkai.png");
        assert!(install_desktop(&dir.join("applications"), Path::new("/usr/bin/orkai"), &icon).unwrap());
        assert!(!install_desktop(&dir.join("applications"), Path::new("/usr/bin/orkai"), &icon).unwrap());
        assert!(dir.join("applications").join(DESKTOP_FILE).is_file());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_bundled_icons_are_the_expected_images() {
        assert_eq!(&ICON_ICO[..4], &[0, 0, 1, 0]);
        let count = usize::from(u16::from_le_bytes([ICON_ICO[4], ICON_ICO[5]]));
        let sizes: Vec<u16> = (0..count)
            .map(|i| match ICON_ICO[6 + i * 16] {
                0 => 256,
                n => u16::from(n),
            })
            .collect();
        assert!(sizes.contains(&16) && sizes.contains(&32) && sizes.contains(&256), "{sizes:?}");
        assert_eq!(&ICON_PNG[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(ICON_PNG[16..20].try_into().unwrap()), 256);
        assert_eq!(u32::from_be_bytes(ICON_PNG[20..24].try_into().unwrap()), 256);
    }
}
