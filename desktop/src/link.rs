use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use orkai::model::{AgentId, AgentRecord};
use orkai::paths::Home;
use orkai::pwa::hub::{Event, Hub};
use orkai::pwa::protocol::PhoneMsg;
use orkai::pwa::server::{Outbox, Peer, welcome};
use orkai::util::lock;
use orkai::worktree;
use serde::Serialize;
use tauri::webview::PageLoadEvent;
use tauri::{Manager, State};

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

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommitFileInfo {
    pub status: String,
    pub path: String,
    pub from: Option<String>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub files: Vec<CommitFileInfo>,
    pub more: usize,
    pub url: Option<String>,
}

fn describe_commit(record: &AgentRecord, hash: &str) -> Result<CommitInfo, String> {
    let dir = match &record.worktree {
        Some(mine) => mine.path.clone(),
        None => worktree::folder(&record.cwd).map_err(|err| err.to_string())?.repo,
    };
    let detail = worktree::commit_detail(&dir, hash).map_err(|err| err.to_string())?;
    Ok(CommitInfo {
        files: detail
            .files
            .into_iter()
            .map(|file| CommitFileInfo { status: file.status.to_string(), path: file.path, from: file.renamed_from })
            .collect(),
        more: detail.more,
        url: detail.url,
    })
}

#[tauri::command(async)]
fn commit_info(link: State<'_, Link>, agent: AgentId, hash: String) -> Result<CommitInfo, String> {
    let record = link.hub().record(agent).ok_or_else(|| format!("agent {agent} is gone"))?;
    describe_commit(&record, &hash)
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
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                webview.state::<Console>().close_all();
            }
        })
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
            commit_info,
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

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn hovering_a_commit_of_an_agent_lists_its_files_from_the_folder_of_the_agent() {
        let fx = fixture("commit");
        let repo = fx.dir.join("project");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "octo@example.com"]);
        git(&repo, &["config", "user.name", "octo"]);
        std::fs::write(repo.join("readme.md"), "hi\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "first"]);
        let store = Store::open(Home::at(fx.dir.join("agents"))).unwrap();
        let record = store
            .create_agent(orkai::model::NewAgent {
                session: "default".into(),
                name: "docs".into(),
                kind: orkai::model::AgentKind::Terminal,
                cwd: repo.clone(),
                task: String::new(),
            })
            .unwrap();

        let info = describe_commit(&record, &git(&repo, &["rev-parse", "--short", "HEAD"])).unwrap();
        assert_eq!(info.files, [CommitFileInfo { status: "A".into(), path: "readme.md".into(), from: None }]);
        assert_eq!((info.more, info.url.as_deref()), (0, None));
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["files"][0]["status"], "A", "the window reads camelCase fields");
        assert!(describe_commit(&record, "--output=/tmp/x").unwrap_err().contains("not a commit hash"));
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
