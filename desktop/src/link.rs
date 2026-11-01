use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use orkai::paths::Home;
use orkai::pwa::hub::{Event, Hub};
use orkai::pwa::protocol::PhoneMsg;
use orkai::pwa::server::{Outbox, Peer, welcome};
use orkai::util::lock;
use tauri::State;

use crate::term::{self, Console};
use crate::updates::{self, Updates};
use crate::{alerts, pair, recover, settings};
use tauri::ipc::{Channel, InvokeResponseBody};

pub struct Link {
    hub: Arc<Hub>,
    generation: Arc<AtomicU64>,
    inbox: Mutex<Option<Sender<Event>>>,
}

struct Webview {
    channel: Channel,
}

impl Outbox for Webview {
    fn deliver(&mut self, plain: &[u8]) -> io::Result<()> {
        let json = String::from_utf8(plain.to_vec()).map_err(io::Error::other)?;
        self.channel.send(InvokeResponseBody::Json(json)).map_err(io::Error::other)
    }
}

impl Link {
    pub(crate) fn new(hub: Arc<Hub>) -> Self {
        Self { hub, generation: Arc::new(AtomicU64::new(0)), inbox: Mutex::new(None) }
    }

    pub fn hub(&self) -> &Hub {
        &self.hub
    }

    fn attach(&self, channel: Channel) -> Result<(), String> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (hub, current) = (Arc::clone(&self.hub), Arc::clone(&self.generation));
        let (ready, inbox) = mpsc::channel();
        thread::Builder::new()
            .name("orkai-desktop-peer".into())
            .spawn(move || {
                let mut webview = Webview { channel };
                if webview.send(&welcome(&hub)).is_err() {
                    return;
                }
                let mut peer = Peer::join(&hub);
                if ready.send(peer.inbox()).is_err() {
                    return;
                }
                while current.load(Ordering::SeqCst) == generation && peer.wait(&mut webview).is_ok() {}
            })
            .map_err(|err| err.to_string())?;
        let inbox = inbox.recv().map_err(|_| "the window could not join Octo".to_string())?;
        *lock(&self.inbox) = Some(inbox);
        Ok(())
    }

    fn send(&self, message: PhoneMsg) -> Result<(), String> {
        let inbox = lock(&self.inbox).clone().ok_or_else(|| "the window is not attached yet".to_string())?;
        inbox.send(Event::Inbound(message)).map_err(|_| "Octo stopped listening to this window".to_string())
    }
}

#[tauri::command]
fn attach(link: State<'_, Link>, channel: Channel) -> Result<(), String> {
    link.attach(channel)
}

#[tauri::command]
fn send(link: State<'_, Link>, message: PhoneMsg) -> Result<(), String> {
    link.send(message)
}

#[tauri::command]
fn home_dir() -> Option<String> {
    orkai::paths::user_home().map(|home| home.display().to_string())
}

#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    if !orkai::termlink::is_web_link(&url) {
        return Err(format!("{url} is not a web link"));
    }
    orkai::browser::open(url);
    Ok(())
}

pub fn run() -> io::Result<()> {
    let hub = Hub::open(&Home::resolve()?)?;
    pair::keep_alive(Arc::clone(&hub));
    recover::on_launch(Arc::clone(&hub));
    let link = Link::new(hub);
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(link)
        .manage(Updates::default())
        .manage(Console::default())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            crate::window::open(app)?;
            updates::on_launch(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            attach,
            send,
            open_link,
            home_dir,
            settings::settings,
            settings::save_settings,
            settings::play_chime,
            settings::check_update,
            alerts::alert,
            pair::pairing,
            pair::set_phone,
            pair::renew_phone,
            updates::update_ready,
            updates::restart_app,
            term::term_open,
            term::term_input,
            term::term_resize,
            term::term_close
        ])
        .run(tauri::generate_context!())
        .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    use orkai::config::Config;
    use orkai::launch::Launcher;
    use orkai::store::Store;
    use serde_json::Value;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(20);

    struct Fixture {
        dir: PathBuf,
        link: Link,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(name: &str) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("orkai-desktop-{name}-{}-{}", std::process::id(), orkai::util::now_ms()));
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        Fixture { link: Link::new(Hub::new(launcher, "testhost".into())), dir }
    }

    fn window() -> (Channel, Receiver<Value>) {
        let (tx, rx) = mpsc::channel();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                let _ = tx.send(serde_json::from_str(&json).unwrap());
            }
            Ok(())
        });
        (channel, rx)
    }

    fn until(rx: &Receiver<Value>, wanted: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + TIMEOUT;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(message) if wanted(&message) => return message,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        panic!("the window never got the expected message");
    }

    fn of_type(kind: &'static str) -> impl Fn(&Value) -> bool {
        move |message| message["type"] == kind
    }

    #[test]
    fn a_window_is_welcomed_and_then_sees_the_overview() {
        let fx = fixture("welcome");
        let (channel, rx) = window();
        fx.link.attach(channel).unwrap();
        let first = rx.recv_timeout(TIMEOUT).unwrap();
        assert_eq!(first["type"], "welcome");
        assert_eq!(first["host"], "testhost");
        let overview = until(&rx, of_type("state"));
        assert_eq!(overview["host"], "testhost");
        assert!(overview["sessions"].as_array().unwrap().iter().any(|session| session["name"] == "default"));
    }

    #[test]
    fn what_the_window_sends_reaches_the_hub_and_comes_back() {
        let fx = fixture("send");
        std::fs::create_dir_all(fx.dir.join("alpha")).unwrap();
        let (channel, rx) = window();
        fx.link.attach(channel).unwrap();
        fx.link.send(PhoneMsg::Folders { path: fx.dir.to_string_lossy().into_owned() }).unwrap();
        let folders = until(&rx, of_type("folders"));
        let names: Vec<&str> =
            folders["dirs"].as_array().unwrap().iter().filter_map(|dir| dir["name"].as_str()).collect();
        assert!(names.contains(&"alpha"), "{names:?}");
    }

    #[test]
    fn a_reloaded_window_replaces_the_previous_one_in_the_hub() {
        let fx = fixture("reload");
        let (first, _first_rx) = window();
        fx.link.attach(first).unwrap();
        assert_eq!(fx.link.hub.phones(), 1);
        let (second, second_rx) = window();
        fx.link.attach(second).unwrap();
        until(&second_rx, of_type("state"));
        let deadline = Instant::now() + TIMEOUT;
        while fx.link.hub.phones() != 1 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fx.link.hub.phones(), 1, "the old window leaves the hub");
    }

    #[test]
    fn only_web_links_leave_the_app() {
        let err = open_link("file:///etc/passwd".into()).unwrap_err();
        assert!(err.contains("not a web link"), "{err}");
        assert!(open_link("javascript:alert(1)".into()).is_err());
    }

    #[test]
    fn a_window_that_never_attached_cannot_send() {
        let fx = fixture("detached");
        let err = fx.link.send(PhoneMsg::Sync).unwrap_err();
        assert!(err.contains("not attached"), "{err}");
    }
}
