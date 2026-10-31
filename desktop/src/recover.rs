use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use orkai::pwa::actions;
use orkai::pwa::hub::{Event, Hub};

const SETTLE_LIMIT: Duration = Duration::from_secs(20);
const SETTLE_POLL: Duration = Duration::from_millis(200);

pub fn on_launch(hub: Arc<Hub>) {
    let _ = thread::Builder::new().name("octo-recover".into()).spawn(move || {
        if wait_until_settled(&hub, SETTLE_LIMIT) {
            recover(&hub);
        }
    });
}

pub fn wait_until_settled(hub: &Hub, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while !hub.settled() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SETTLE_POLL);
    }
    true
}

fn recover(hub: &Arc<Hub>) -> usize {
    let (reply, notices) = mpsc::channel();
    let count = actions::recover_all(hub, &reply);
    drop(reply);
    for event in notices {
        if let Event::Reply(message) = event {
            hub.broadcast(&message);
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use orkai::config::Config;
    use orkai::launch::Launcher;
    use orkai::paths::Home;
    use orkai::store::Store;

    use super::*;

    fn hub(name: &str) -> (std::path::PathBuf, Arc<Hub>) {
        let dir =
            std::env::temp_dir().join(format!("octo-recover-{name}-{}-{}", std::process::id(), orkai::util::now_ms()));
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        (dir, Hub::new(launcher, "testhost".into()))
    }

    #[test]
    fn a_hub_nobody_watches_never_settles_so_nothing_is_recovered_blindly() {
        let (dir, hub) = hub("unwatched");
        let started = Instant::now();
        assert!(!wait_until_settled(&hub, Duration::from_millis(400)));
        assert!(started.elapsed() >= Duration::from_millis(400));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_settled_hub_without_interrupted_agents_recovers_nothing() {
        let (dir, hub) = hub("calm");
        let (tx, _rx) = mpsc::channel();
        hub.join(tx);
        assert!(wait_until_settled(&hub, Duration::from_secs(10)));
        assert_eq!(recover(&hub), 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
