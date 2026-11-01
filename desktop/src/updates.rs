use std::path::Path;
use std::sync::Mutex;
use std::thread;

use orkai::update::{self, BUILD_COMMIT, DISABLE_ENV, Outcome, Source};
use orkai::util::lock;
use tauri::{AppHandle, Emitter, Manager, State};

pub const ASSET_PREFIX: &str = "desktop-";
pub const APP_ASSET_PREFIX: &str = "desktop-app-";
pub const READY_EVENT: &str = "update-ready";

#[derive(Default)]
pub struct Updates {
    ready: Mutex<Option<String>>,
}

impl Updates {
    pub fn ready(&self) -> Option<String> {
        lock(&self.ready).clone()
    }

    fn mark(&self, version: &str) {
        *lock(&self.ready) = Some(version.to_string());
    }
}

pub fn target() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("x86_64-pc-windows-msvc")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("x86_64-apple-darwin")
    } else {
        None
    }
}

pub fn asset_key(target: &str) -> String {
    format!("{ASSET_PREFIX}{target}")
}

pub fn app_asset_key(target: &str) -> String {
    format!("{APP_ASSET_PREFIX}{target}")
}

fn replaces_its_bundle(exe: &Path) -> bool {
    cfg!(target_os = "macos") && update::app_bundle(exe).is_some()
}

pub fn updates_itself() -> bool {
    BUILD_COMMIT.is_some() && target().is_some() && std::env::var_os(DISABLE_ENV).is_none()
}

pub fn install_newer(source: &Source, exe: &Path) -> Result<Option<String>, String> {
    let commit = BUILD_COMMIT.ok_or("a build made on this computer does not update itself")?;
    let target = target().ok_or("there is no prebuilt Octo app for this platform")?;
    let manifest = source.manifest().map_err(|err| err.to_string())?;
    let version = env!("CARGO_PKG_VERSION");
    let outcome = if replaces_its_bundle(exe) {
        update::apply_with(source, &manifest, Some(commit), version, &app_asset_key(target), |archive| {
            update::install_app(exe, archive)
        })
    } else {
        update::apply(source, &manifest, exe, Some(commit), version, &asset_key(target))
    }
    .map_err(|err| err.to_string())?;
    Ok(matches!(outcome, Outcome::Updated { .. }).then_some(manifest.version))
}

pub fn install_and_announce(app: &AppHandle) -> Result<Option<String>, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let installed = install_newer(&Source::from_env(), &exe)?;
    if let Some(version) = installed.as_deref() {
        app.state::<Updates>().mark(version);
        let _ = app.emit(READY_EVENT, version);
    }
    Ok(installed)
}

pub fn on_launch(app: &AppHandle) {
    if let Ok(exe) = std::env::current_exe() {
        update::cleanup(&exe);
        update::cleanup_app(&exe);
    }
    if !updates_itself() {
        return;
    }
    let app = app.clone();
    let _ = thread::Builder::new().name("orkai-desktop-update".into()).spawn(move || {
        let _ = install_and_announce(&app);
    });
}

#[tauri::command]
pub fn update_ready(updates: State<'_, Updates>) -> Option<String> {
    updates.ready()
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use orkai::update::{Asset, Manifest, Plan, plan};

    use super::*;

    const RUNNING: &str = "4645e00aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn manifest(version: &str, keys: &[&str]) -> Manifest {
        let assets: BTreeMap<String, Asset> = keys
            .iter()
            .map(|key| ((*key).to_string(), Asset { name: format!("orkai-{key}"), sha256: "00".repeat(32), size: 10 }))
            .collect();
        Manifest {
            commit: "9238b7abbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            version: version.into(),
            built_at: String::new(),
            assets,
        }
    }

    #[test]
    fn the_app_has_the_version_of_the_orkai_it_is_built_with() {
        let core = orkai::update::VERSION.split_whitespace().next().unwrap();
        assert_eq!(env!("CARGO_PKG_VERSION"), core, "raise desktop/Cargo.toml together with Cargo.toml");
    }

    #[test]
    fn the_app_downloads_its_own_file_of_a_newer_release_and_never_the_cli() {
        let key = asset_key("x86_64-pc-windows-msvc");
        assert_eq!(key, "desktop-x86_64-pc-windows-msvc");
        let both = manifest("9.0.0", &["x86_64-pc-windows-msvc", &key]);
        match plan(&both, Some(RUNNING), env!("CARGO_PKG_VERSION"), &key) {
            Plan::Install { asset, .. } => assert_eq!(asset.name, "orkai-desktop-x86_64-pc-windows-msvc"),
            other => panic!("expected an install, got {other:?}"),
        }
        let cli_only = manifest("9.0.0", &["x86_64-pc-windows-msvc"]);
        assert_eq!(
            plan(&cli_only, Some(RUNNING), env!("CARGO_PKG_VERSION"), &key),
            Plan::NoAsset,
            "a release without the app"
        );
        let same = manifest(env!("CARGO_PKG_VERSION"), &[&key]);
        assert_eq!(plan(&same, Some(RUNNING), env!("CARGO_PKG_VERSION"), &key), Plan::UpToDate);
    }

    #[test]
    fn a_mac_app_downloads_its_whole_bundle_while_an_older_one_keeps_its_binary() {
        let target = "aarch64-apple-darwin";
        let app = app_asset_key(target);
        assert_eq!(app, "desktop-app-aarch64-apple-darwin");
        let both = manifest("9.0.0", &[&asset_key(target), &app]);
        match plan(&both, Some(RUNNING), env!("CARGO_PKG_VERSION"), &app) {
            Plan::Install { asset, .. } => assert_eq!(asset.name, "orkai-desktop-app-aarch64-apple-darwin"),
            other => panic!("expected the bundle, got {other:?}"),
        }
        match plan(&both, Some(RUNNING), env!("CARGO_PKG_VERSION"), &asset_key(target)) {
            Plan::Install { asset, .. } => assert_eq!(asset.name, "orkai-desktop-aarch64-apple-darwin"),
            other => panic!("expected the binary, got {other:?}"),
        }
        let binary_only = manifest("9.0.0", &[&asset_key(target)]);
        assert_eq!(plan(&binary_only, Some(RUNNING), env!("CARGO_PKG_VERSION"), &app), Plan::NoAsset);
    }

    #[test]
    fn only_a_mac_app_replaces_its_whole_bundle() {
        let in_bundle = Path::new("/Applications/Octo.app/Contents/MacOS/octo");
        assert_eq!(replaces_its_bundle(in_bundle), cfg!(target_os = "macos"));
        assert!(!replaces_its_bundle(Path::new("/usr/local/bin/octo")));
    }

    #[test]
    fn every_platform_the_release_builds_has_a_target() {
        assert!(target().is_some(), "the CI builds the app for this platform, so it must know its own file");
    }

    #[test]
    fn a_local_build_never_replaces_itself() {
        if BUILD_COMMIT.is_none() {
            assert!(!updates_itself());
            let err = install_newer(&Source::at("http://127.0.0.1:9"), Path::new("orkai-desktop")).unwrap_err();
            assert!(err.contains("does not update itself"), "{err}");
        }
    }
}
