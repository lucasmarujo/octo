use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use orkai::client;
use orkai::config::Config;
use orkai::launch::{Launcher, kill_process_tree, process_is_holder, spawn_holder, wait_for_runtime};
use orkai::migrate::{Adoption, adopt};
use orkai::model::{AgentId, AgentKind, LaunchSpec, NewAgent};
use orkai::paths::{HOME_DIR, Home, LEGACY_HOME_DIR, located_in};
use orkai::protocol::ClientMsg;
use orkai::pwa::bridge;
use orkai::pwa::tunnel::TunnelKind;
use orkai::store::Store;
use orkai::util::now_ms;

const TIMEOUT: Duration = Duration::from_secs(90);
const STEP: Duration = Duration::from_millis(50);

struct Fixture {
    base: PathBuf,
    home: PathBuf,
    legacy: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.home).is_ok_and(|meta| meta.file_type().is_symlink()) {
            let _ = if cfg!(windows) { std::fs::remove_dir(&self.home) } else { std::fs::remove_file(&self.home) };
        }
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn fixture(tag: &str) -> Fixture {
    let base = std::env::temp_dir().join(format!("octo-home-{tag}-{}-{}", std::process::id(), now_ms()));
    std::fs::create_dir_all(&base).unwrap();
    Fixture { home: base.join(HOME_DIR), legacy: base.join(LEGACY_HOME_DIR), base }
}

fn until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(STEP);
    }
    done()
}

fn draft(name: &str, cwd: &Path) -> NewAgent {
    NewAgent {
        session: "default".into(),
        name: name.into(),
        kind: AgentKind::Terminal,
        cwd: cwd.to_path_buf(),
        task: String::new(),
    }
}

fn start_shell(launcher: &Launcher, cwd: &Path) -> AgentId {
    let store = launcher.store();
    let id = store.create_agent(draft("shell", cwd)).unwrap().id;
    let (program, args) = if cfg!(windows) {
        (PathBuf::from("cmd.exe"), vec!["/q".to_string()])
    } else {
        (PathBuf::from("/bin/sh"), Vec::new())
    };
    let spec = LaunchSpec {
        kind: AgentKind::Terminal,
        program,
        args,
        cwd: cwd.to_path_buf(),
        env: vec![("TERM".into(), "xterm-256color".into())],
        env_remove: Vec::new(),
        initial_input: None,
        rows: 24,
        cols: 80,
        test: None,
    };
    store.write_launch(id, &spec).unwrap();
    spawn_holder(store.home(), id, launcher.holder_exe()).unwrap();
    wait_for_runtime(store, id, TIMEOUT).unwrap();
    id
}

fn end_agents_host(home: &Home) {
    if let Ok(text) = std::fs::read_to_string(home.agents_host_file())
        && let Some(pid) = serde_json::from_str::<serde_json::Value>(&text).unwrap()["pid"].as_u64()
    {
        kill_process_tree(pid as u32);
    }
}

#[test]
fn a_running_agent_never_notices_the_data_folder_getting_its_new_name() {
    let fx = fixture("agent");
    let work = fx.base.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let old = Store::open(Home::at(&fx.legacy)).unwrap();
    let launcher = Launcher::new(old.clone(), Config::default(), Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let id = start_shell(&launcher, &work);
    let runtime = old.read_runtime(id).unwrap().unwrap();
    assert!(process_is_holder(runtime.holder_pid));

    assert_eq!(adopt(&fx.base, process_is_holder).unwrap(), Adoption::Linked);
    assert_eq!(located_in(&fx.base), fx.home, "the app already works under the new name");
    let new = Store::open(Home::at(&fx.home)).unwrap();
    assert_eq!(new.load_agent(id).unwrap().name, "shell", "and sees the same agent");
    assert_eq!(new.read_runtime(id).unwrap().unwrap(), runtime);
    client::deliver(&runtime, &ClientMsg::Seen).expect("the holder still answers");
    assert_eq!(new.read_exit(id).unwrap(), None);

    let later = new.create_agent(draft("later", &work)).unwrap();
    assert!(Home::at(&fx.legacy).agent_file(later.id).is_file(), "written through the link");

    client::deliver(&runtime, &ClientMsg::Kill).unwrap();
    assert!(until(|| new.read_exit(id).is_ok_and(|exit| exit.is_some())), "the agent ends");
    assert!(until(|| !process_is_holder(runtime.holder_pid)), "and its holder too");
    end_agents_host(new.home());
    let moved = until(|| adopt(&fx.base, process_is_holder).is_ok_and(|done| done == Adoption::Moved));
    assert!(moved, "with nothing left running the folder takes the new name");

    assert!(!std::fs::symlink_metadata(&fx.home).unwrap().file_type().is_symlink());
    assert!(!fx.legacy.exists());
    let names: Vec<String> = new.list_agents().unwrap().into_iter().map(|record| record.name).collect();
    assert_eq!(names, ["shell", "later"]);
    assert_eq!(adopt(&fx.base, process_is_holder).unwrap(), Adoption::Settled);
}

#[test]
fn the_phone_bridge_steps_aside_for_the_move_and_the_pairing_goes_with_the_folder() {
    let fx = fixture("bridge");
    let old = Home::at(&fx.legacy);
    Store::open(old.clone()).unwrap();
    let secret = bridge::load_secret(&old).unwrap();
    let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port();
    let exe = Path::new(env!("CARGO_BIN_EXE_orkai"));
    bridge::start(&old, exe, TunnelKind::Tailscale, port).unwrap();
    assert!(bridge::wait_until(&old, TIMEOUT, |info| info.is_some()), "the bridge is up");

    let moved = until(|| adopt(&fx.base, process_is_holder).is_ok_and(|done| done == Adoption::Moved));
    if !moved {
        bridge::stop(&old).unwrap();
    }
    assert!(moved, "the bridge stops once it is done opening its tunnel");
    assert!(!fx.legacy.exists(), "nothing of the bridge holds the old folder");
    let new = Home::at(&fx.home);
    assert_eq!(bridge::load_secret(&new).unwrap(), secret, "paired phones stay paired");
}
