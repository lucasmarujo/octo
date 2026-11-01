#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use orkai::client;
use orkai::config::Config;
use orkai::launch::{Launcher, process_is_holder, spawn_holder, wait_for_runtime};
use orkai::model::{AgentKind, LaunchSpec, NewAgent};
use orkai::paths::Home;
use orkai::protocol::ClientMsg;
use orkai::store::Store;
use orkai::update::{app_bundle, install};
use orkai::util::now_ms;

const SIGNED_APP_ENV: &str = "OCTO_SIGNED_APP";
const LEGACY_UPDATE_ENV: &str = "OCTO_LEGACY_UPDATE";
const BUNDLE_EXECUTABLE: &str = "Contents/MacOS/octo";
const CODESIGN: &str = "/usr/bin/codesign";
const SHELL: &str = "/bin/sh";
const GREETING: &str = "started-from-the-app-bundle";
const TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(100);
const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>octo</string>
  <key>CFBundleIdentifier</key><string>dev.marujo.octo</string>
  <key>CFBundleName</key><string>Octo</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict>
</plist>
"#;

struct Fixture {
    dir: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn codesign(args: &[&str], path: &Path) -> bool {
    Command::new(CODESIGN).args(args).arg(path).status().unwrap().success()
}

fn unsigned_app(dir: &Path) -> PathBuf {
    let app = dir.join("Octo.app");
    let exe = app.join(BUNDLE_EXECUTABLE);
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_octo"), &exe).unwrap();
    std::fs::write(app.join("Contents/Info.plist"), INFO_PLIST).unwrap();
    app
}

fn signed_app(dir: &Path) -> PathBuf {
    if let Some(app) = std::env::var_os(SIGNED_APP_ENV) {
        return PathBuf::from(app);
    }
    let app = unsigned_app(dir);
    assert!(codesign(&["--force", "--options", "runtime", "--sign", "-"], &app));
    app
}

fn legacy_update() -> PathBuf {
    std::env::var_os(LEGACY_UPDATE_ENV).map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_octo")), PathBuf::from)
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(POLL);
    }
}

#[test]
fn an_app_signed_ad_hoc_still_starts_after_it_swaps_in_the_binary_of_a_newer_release() {
    let fx =
        Fixture { dir: std::env::temp_dir().join(format!("octo-macos-legacy-{}-{}", std::process::id(), now_ms())) };
    let app = unsigned_app(&fx.dir);
    assert!(codesign(&["--force", "--sign", "-"], &app), "Octo up to 0.2.9 was signed ad hoc");
    let exe = app.join(BUNDLE_EXECUTABLE);
    install(&exe, &std::fs::read(legacy_update()).unwrap()).unwrap();
    let started = Command::new(&exe).arg("--version").output().unwrap();
    assert!(
        started.status.success(),
        "macOS refused the updated app as it started ({}): {}",
        started.status,
        String::from_utf8_lossy(&started.stderr)
    );
    assert!(
        String::from_utf8_lossy(&started.stdout).starts_with("octo "),
        "{}",
        String::from_utf8_lossy(&started.stdout)
    );
}

#[test]
fn an_agent_starts_from_the_signed_app_bundle() {
    let fx = Fixture { dir: std::env::temp_dir().join(format!("octo-macos-app-{}-{}", std::process::id(), now_ms())) };
    let app = signed_app(&fx.dir);
    let store = Store::open(Home::at(fx.dir.join("Application Support").join("orkai"))).unwrap();
    let launcher = Launcher::new(store.clone(), Config::default(), &app.join(BUNDLE_EXECUTABLE)).unwrap();
    let holder = launcher.holder_exe();
    let staged = app_bundle(holder).expect("the holder runs from a copy of the whole app bundle");
    assert!(staged.starts_with(store.home().bin_dir()), "{}", staged.display());
    assert!(
        codesign(&["--verify", "--strict"], staged),
        "the copied holder keeps a valid signature, otherwise macOS kills it as it starts"
    );
    let cwd = fx.dir.join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let record = store
        .create_agent(NewAgent {
            session: "default".into(),
            name: "mac".into(),
            kind: AgentKind::Terminal,
            cwd: cwd.clone(),
            task: String::new(),
        })
        .unwrap();
    let spec = LaunchSpec {
        kind: AgentKind::Terminal,
        program: PathBuf::from(SHELL),
        args: vec!["-c".into(), format!("echo {GREETING}; sleep 60")],
        cwd,
        env: Vec::new(),
        env_remove: Vec::new(),
        initial_input: None,
        rows: 24,
        cols: 80,
        test: None,
    };
    store.write_launch(record.id, &spec).unwrap();
    spawn_holder(store.home(), record.id, holder).unwrap();
    let runtime = wait_for_runtime(&store, record.id, TIMEOUT).expect("the holder copied from the app reports in");
    assert!(process_is_holder(runtime.holder_pid), "a holder inside a bundle is still known by its name");
    wait_until(|| {
        client::peek(&runtime).is_ok_and(|snapshot| String::from_utf8_lossy(&snapshot.data).contains(GREETING))
    });
    client::deliver(&runtime, &ClientMsg::Kill).unwrap();
    wait_until(|| store.read_exit(record.id).unwrap().is_some());
}
