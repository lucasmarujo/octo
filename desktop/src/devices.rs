use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use orkai::fsutil;
use orkai::model::AgentState;
use orkai::paths::Home;
use orkai::pwa::crypto::{self, Secret};
use orkai::pwa::protocol::AgentView;
use orkai::pwa::protocol::PhoneMsg;
use orkai::pwa::server::Outbox;
use orkai::util::{lock, now_ms, random_bytes};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::link::Link;
use crate::remote::{self, Address, Remote, Status};
use crate::ssh::{Server, Tunnel};

pub const MAX_DEVICES: usize = 16;
const FILE: &str = "devices.json";
const MAX_NAME: usize = 60;
const ID_BYTES: usize = 16;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Saved {
    id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    ssh: Option<Server>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    added_at_ms: u64,
}

impl Saved {
    fn target(&self) -> Option<(Address, Secret)> {
        Some((Address::parse(&self.url)?, crypto::decode_secret(&self.key)?))
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub address: String,
    pub kind: Kind,
    #[serde(flatten)]
    pub status: Status,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Link,
    Ssh,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Here {
    pub agents: usize,
    pub working: usize,
    pub waiting: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub here: Here,
    pub devices: Vec<DeviceInfo>,
}

pub fn here(agents: &[AgentView]) -> Here {
    Here {
        agents: agents.len(),
        working: agents.iter().filter(|agent| agent.starting || agent.state == AgentState::Working).count(),
        waiting: agents.iter().filter(|agent| !agent.starting && agent.state == AgentState::Paused).count(),
    }
}

#[derive(Default)]
struct Inner {
    saved: Vec<Saved>,
    links: BTreeMap<String, Remote>,
    tunnels: BTreeMap<String, Tunnel>,
}

pub struct Devices {
    path: PathBuf,
    inner: Mutex<Inner>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn new_id() -> Result<String, String> {
    random_bytes::<ID_BYTES>().map(|bytes| hex(&bytes)).map_err(|err| err.to_string())
}

fn clean_name(name: &str) -> Option<String> {
    let clean: String = name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    let clean = clean.trim();
    (!clean.is_empty()).then(|| clean.to_string())
}

fn describe(saved: &Saved, status: Status) -> DeviceInfo {
    let address = match &saved.ssh {
        Some(server) => server.label(),
        None => Address::parse(&saved.url).map(|address| address.authority().to_string()).unwrap_or_default(),
    };
    let kind = if saved.ssh.is_some() { Kind::Ssh } else { Kind::Link };
    let host = status.host.clone().or_else(|| saved.host.clone());
    let named = match (&saved.ssh, host) {
        (Some(server), Some(host)) => Some(format!("{}@{host}", server.user)),
        (_, host) => host,
    };
    let name = saved.name.clone().or(named).unwrap_or_else(|| address.clone());
    DeviceInfo { id: saved.id.clone(), name, address, kind, status }
}

impl Inner {
    fn link(&mut self, saved: &Saved, password: Option<String>) -> Result<(), String> {
        if let Some(server) = &saved.ssh {
            let remote = Remote::parked(new_id()?).map_err(|err| err.to_string())?;
            let tunnel = Tunnel::open(server.clone(), remote.clone(), password).map_err(|err| err.to_string())?;
            self.tunnels.insert(saved.id.clone(), tunnel);
            self.links.insert(saved.id.clone(), remote);
            return Ok(());
        }
        let (address, secret) = saved.target().ok_or_else(|| "the saved link of this device is broken".to_string())?;
        let remote = Remote::start(address, secret, new_id()?).map_err(|err| err.to_string())?;
        self.links.insert(saved.id.clone(), remote);
        Ok(())
    }

    fn info(&self, saved: &Saved) -> DeviceInfo {
        describe(saved, self.links.get(&saved.id).map(Remote::status).unwrap_or_default())
    }

    fn remote(&self, id: &str) -> Result<&Remote, String> {
        self.links.get(id).ok_or_else(|| "this device is not on the list anymore".to_string())
    }
}

impl Devices {
    pub fn open(home: &Home) -> Self {
        let path = home.root().join(FILE);
        let saved: Vec<Saved> = fsutil::read_json_opt(&path).ok().flatten().unwrap_or_default();
        let mut inner = Inner { saved, ..Inner::default() };
        for saved in inner.saved.clone() {
            let _ = inner.link(&saved, None);
        }
        Self { path, inner: Mutex::new(inner) }
    }

    fn keep(&self, inner: &Inner) -> Result<(), String> {
        fsutil::write_json_atomic(&self.path, &inner.saved).map_err(|err| err.to_string())
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        let mut inner = lock(&self.inner);
        let listed: Vec<DeviceInfo> = inner.saved.iter().map(|saved| inner.info(saved)).collect();
        let mut learned = false;
        for (saved, info) in inner.saved.iter_mut().zip(&listed) {
            if info.status.host.is_some() && saved.host != info.status.host {
                saved.host.clone_from(&info.status.host);
                learned = true;
            }
        }
        if learned {
            let _ = self.keep(&inner);
        }
        listed
    }

    pub fn add(&self, link: &str) -> Result<DeviceInfo, String> {
        let (address, secret) = remote::parse_link(link).ok_or_else(|| {
            "that is not a pairing link of Octo; copy it from Pair your phone on the other computer".to_string()
        })?;
        let (url, key) = (address.url(), crypto::encode(&secret));
        let mut inner = lock(&self.inner);
        let link = |saved: &Saved, same: bool| saved.ssh.is_none() && same;
        let known = inner
            .saved
            .iter()
            .position(|saved| link(saved, saved.key == key))
            .or_else(|| inner.saved.iter().position(|saved| link(saved, saved.url == url)));
        let saved = match known {
            Some(at) => {
                inner.saved[at].url = url;
                inner.saved[at].key = key;
                inner.saved[at].clone()
            }
            None => {
                if inner.saved.len() >= MAX_DEVICES {
                    return Err(format!("Octo keeps up to {MAX_DEVICES} devices; forget one first"));
                }
                let saved = Saved { id: new_id()?, url, key, ssh: None, name: None, host: None, added_at_ms: now_ms() };
                inner.saved.push(saved.clone());
                saved
            }
        };
        self.keep(&inner)?;
        inner.link(&saved, None)?;
        Ok(inner.info(&saved))
    }

    pub fn add_server(&self, server: Server, password: Option<String>) -> Result<DeviceInfo, String> {
        let mut inner = lock(&self.inner);
        let saved = match inner.saved.iter().find(|saved| saved.ssh.as_ref() == Some(&server)) {
            Some(known) => known.clone(),
            None => {
                if inner.saved.len() >= MAX_DEVICES {
                    return Err(format!("Octo keeps up to {MAX_DEVICES} devices; forget one first"));
                }
                let saved = Saved {
                    id: new_id()?,
                    url: String::new(),
                    key: String::new(),
                    ssh: Some(server),
                    name: None,
                    host: None,
                    added_at_ms: now_ms(),
                };
                inner.saved.push(saved.clone());
                saved
            }
        };
        self.keep(&inner)?;
        inner.tunnels.remove(&saved.id);
        inner.link(&saved, password.filter(|password| !password.is_empty()))?;
        Ok(inner.info(&saved))
    }

    pub fn unlock(&self, id: &str, password: String) -> Result<(), String> {
        let inner = lock(&self.inner);
        let tunnel = inner.tunnels.get(id).ok_or_else(|| "this device is not a server reached by ssh".to_string())?;
        tunnel.unlock(password);
        Ok(())
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<(), String> {
        let mut inner = lock(&self.inner);
        let saved = inner
            .saved
            .iter_mut()
            .find(|saved| saved.id == id)
            .ok_or_else(|| "this device is not on the list anymore".to_string())?;
        saved.name = clean_name(name);
        self.keep(&inner)
    }

    pub fn forget(&self, id: &str) -> Result<(), String> {
        let mut inner = lock(&self.inner);
        inner.saved.retain(|saved| saved.id != id);
        inner.tunnels.remove(id);
        inner.links.remove(id);
        self.keep(&inner)
    }

    pub fn retry(&self, id: &str) -> Result<(), String> {
        let inner = lock(&self.inner);
        match inner.tunnels.get(id) {
            Some(tunnel) => {
                tunnel.retry();
                Ok(())
            }
            None => inner.remote(id).map(Remote::retry),
        }
    }

    pub fn watch(&self, id: &str, window: Box<dyn Outbox + Send>) -> Result<(), String> {
        let inner = lock(&self.inner);
        for (other, remote) in &inner.links {
            if other != id {
                remote.unwatch();
            }
        }
        inner.remote(id).map(|remote| remote.watch(window))
    }

    pub fn unwatch(&self) {
        for remote in lock(&self.inner).links.values() {
            remote.unwatch();
        }
    }

    pub fn send(&self, id: &str, message: &PhoneMsg) -> Result<(), String> {
        lock(&self.inner).remote(id)?.send(message)
    }
}

#[tauri::command]
pub fn devices(link: State<'_, Link>) -> Listing {
    Listing { here: here(&link.hub().agent_views()), devices: link.devices().list() }
}

#[tauri::command]
pub fn add_device(link: State<'_, Link>, pairing: String) -> Result<DeviceInfo, String> {
    link.devices().add(&pairing)
}

#[tauri::command]
pub fn add_server(
    link: State<'_, Link>,
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
) -> Result<DeviceInfo, String> {
    link.devices().add_server(Server::parse(&host, port, &user)?, password)
}

#[tauri::command]
pub fn unlock_device(link: State<'_, Link>, id: String, password: String) -> Result<(), String> {
    link.devices().unlock(&id, password)
}

#[tauri::command]
pub fn rename_device(link: State<'_, Link>, id: String, name: String) -> Result<(), String> {
    link.devices().rename(&id, &name)
}

#[tauri::command]
pub fn forget_device(link: State<'_, Link>, id: String) -> Result<(), String> {
    link.devices().forget(&id)
}

#[tauri::command]
pub fn retry_device(link: State<'_, Link>, id: String) -> Result<(), String> {
    link.devices().retry(&id)
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use orkai::pwa::bridge;

    use super::*;
    use crate::remote::Phase;
    use crate::remote::tests::{other_octo, until, window};

    const TIMEOUT: Duration = Duration::from_secs(20);

    struct Desk {
        dir: PathBuf,
        home: Home,
    }

    impl Drop for Desk {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn desk(name: &str) -> Desk {
        let dir = std::env::temp_dir().join(format!("octo-devices-{name}-{}-{}", std::process::id(), now_ms()));
        let home = Home::at(dir.join("home"));
        home.ensure().unwrap();
        Desk { dir, home }
    }

    fn fake_link(host: &str) -> String {
        bridge::pairing_link(&format!("https://{host}"), &crypto::secret().unwrap())
    }

    fn live(devices: &Devices, id: &str) -> DeviceInfo {
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if let Some(info) =
                devices.list().into_iter().find(|info| info.id == id && info.status.phase == Phase::Live)
            {
                return info;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the device never came up");
    }

    #[test]
    fn an_added_device_is_listed_by_its_host_and_survives_a_restart_without_showing_its_key() {
        let desk = desk("add");
        let octo = other_octo("add", "macbook");
        let devices = Devices::open(&desk.home);
        assert!(devices.list().is_empty());
        let added = devices.add(&octo.link()).unwrap();
        assert_eq!(added.address, format!("127.0.0.1:{}", octo.port));
        let info = live(&devices, &added.id);
        assert_eq!(info.name, "macbook", "a device is called by the name of its computer");
        let shown = serde_json::to_string(&devices.list()).unwrap();
        assert!(!shown.contains(&crypto::encode(&octo.secret)), "the key never reaches the window");
        assert!(shown.contains(r#""phase":"live""#), "{shown}");
        drop(devices);

        let reopened = Devices::open(&desk.home);
        let again = reopened.list();
        assert_eq!(again.len(), 1);
        assert_eq!((again[0].id.as_str(), again[0].name.as_str()), (added.id.as_str(), "macbook"));
        live(&reopened, &added.id);
    }

    #[test]
    fn a_device_is_renamed_and_forgotten() {
        let desk = desk("rename");
        let devices = Devices::open(&desk.home);
        let added = devices.add(&fake_link("octo.example")).unwrap();
        assert_eq!(added.name, "octo.example", "before it answers a device is called by its address");
        devices.rename(&added.id, "  Studio\u{7} Mac  ").unwrap();
        assert_eq!(devices.list()[0].name, "Studio Mac");
        devices.rename(&added.id, "   ").unwrap();
        assert_eq!(devices.list()[0].name, "octo.example", "an empty name goes back to the default");
        assert!(devices.rename("nope", "x").unwrap_err().contains("not on the list"));
        devices.forget(&added.id).unwrap();
        assert!(devices.list().is_empty());
        assert!(Devices::open(&desk.home).list().is_empty());
        assert!(devices.send(&added.id, &PhoneMsg::Sync).unwrap_err().contains("not on the list"));
    }

    #[test]
    fn pasting_a_new_link_of_a_known_device_updates_it_instead_of_adding_another() {
        let desk = desk("update");
        let devices = Devices::open(&desk.home);
        let secret = crypto::secret().unwrap();
        let first = devices.add(&bridge::pairing_link("https://old-tunnel.example", &secret)).unwrap();
        devices.rename(&first.id, "Mac").unwrap();
        let moved = devices.add(&bridge::pairing_link("https://new-tunnel.example", &secret)).unwrap();
        assert_eq!(
            (moved.id.as_str(), moved.address.as_str(), moved.name.as_str()),
            (first.id.as_str(), "new-tunnel.example", "Mac")
        );
        let renewed = devices.add(&fake_link("new-tunnel.example")).unwrap();
        assert_eq!(renewed.id, first.id, "a new code at the same address is the same device");
        assert_eq!(devices.list().len(), 1);
    }

    #[test]
    fn a_server_reached_by_ssh_is_kept_without_its_password_and_is_one_device_however_often_it_is_added() {
        let desk = desk("ssh");
        let devices = Devices::open(&desk.home);
        let server = Server::parse("127.0.0.1", 1, "deploy").unwrap();
        let added = devices.add_server(server.clone(), Some("s3cret-never-saved".into())).unwrap();
        assert_eq!(
            (added.kind, added.address.as_str(), added.name.as_str()),
            (Kind::Ssh, "deploy@127.0.0.1:1", "deploy@127.0.0.1:1")
        );
        let answered = Status { host: Some("build-01".into()), ..Status::default() };
        let saved = Saved {
            id: added.id.clone(),
            url: String::new(),
            key: String::new(),
            ssh: Some(server.clone()),
            name: None,
            host: None,
            added_at_ms: 0,
        };
        assert_eq!(
            describe(&saved, answered).name,
            "deploy@build-01",
            "two users of one server stay apart in the list"
        );
        let again = devices.add_server(server, None).unwrap();
        assert_eq!(again.id, added.id);
        assert_eq!(devices.list().len(), 1);
        let kept = std::fs::read_to_string(desk.home.root().join(FILE)).unwrap();
        assert!(kept.contains(r#""host": "127.0.0.1""#) && kept.contains(r#""user": "deploy""#), "{kept}");
        assert!(!kept.contains("s3cret"), "the password never reaches the disk");
        assert!(!serde_json::to_string(&devices.list()).unwrap().contains("s3cret"));
        devices.unlock(&added.id, "another".into()).unwrap();
        devices.retry(&added.id).unwrap();
        let link = devices.add(&fake_link("octo.example")).unwrap();
        assert_eq!(link.kind, Kind::Link);
        assert!(devices.unlock(&link.id, "x".into()).unwrap_err().contains("not a server"));
        drop(devices);
        let reopened = Devices::open(&desk.home);
        assert_eq!(reopened.list().iter().map(|info| info.kind).collect::<Vec<_>>(), [Kind::Ssh, Kind::Link]);
        reopened.forget(&added.id).unwrap();
        assert_eq!(reopened.list().len(), 1);
    }

    #[test]
    fn a_bad_link_is_refused_and_the_list_is_bounded() {
        let desk = desk("bounds");
        let devices = Devices::open(&desk.home);
        assert!(devices.add("hello").unwrap_err().contains("not a pairing link"));
        assert!(devices.add("https://octo.example/").unwrap_err().contains("not a pairing link"));
        for index in 0..MAX_DEVICES {
            devices.add(&fake_link(&format!("octo-{index}.example"))).unwrap();
        }
        assert!(devices.add(&fake_link("one-too-many.example")).unwrap_err().contains("forget one first"));
        assert_eq!(devices.list().len(), MAX_DEVICES);
    }

    #[test]
    fn only_the_device_on_screen_talks_to_the_window() {
        let desk = desk("watch");
        let (one, two) = (other_octo("watch-one", "one"), other_octo("watch-two", "two"));
        let devices = Devices::open(&desk.home);
        let (first, second) = (devices.add(&one.link()).unwrap(), devices.add(&two.link()).unwrap());
        live(&devices, &first.id);
        live(&devices, &second.id);
        let (seen, rx) = window();
        devices.watch(&first.id, seen).unwrap();
        assert_eq!(until(&rx, |message| message["type"] == "welcome")["host"], "one");
        let (seen, rx) = window();
        devices.watch(&second.id, seen).unwrap();
        assert_eq!(until(&rx, |message| message["type"] == "welcome")["host"], "two");
        devices.send(&second.id, &PhoneMsg::Sync).unwrap();
        assert_eq!(until(&rx, |message| message["type"] == "state")["host"], "two");
        assert!(devices.watch("nope", window().0).unwrap_err().contains("not on the list"));
    }
}
