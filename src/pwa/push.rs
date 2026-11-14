use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::model::AgentId;
use crate::notify::Alert;
use crate::paths::Home;
use crate::pwa::bridge;
use crate::pwa::protocol::Push;

pub const FILE: &str = "pwa-push.json";
pub const EXPO_PUSH_URL: &str = "https://exp.host/--/api/v2/push/send";
pub const MAX_PHONES: usize = 16;
pub const BATCH: usize = 100;
const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_ANSWER_BYTES: u64 = 256 * 1024;
const MAX_ID: usize = 64;
const MAX_TOKEN: usize = 128;
const TOKEN_PREFIXES: [&str; 2] = ["ExponentPushToken[", "ExpoPushToken["];
const TOKEN_END: char = ']';
const CHANNEL: &str = "agents";
const SOUND: &str = "default";
const PRIORITY: &str = "high";
const FAILED: &str = "error";
const UNREGISTERED: &str = "DeviceNotRegistered";
pub const MOVED_BODY: &str = "Octo is back online at a new address. Open the app to reconnect.";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Phone {
    pub client: String,
    pub token: String,
    pub device: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub device: String,
    pub agent: AgentId,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Moved {
    pub device: String,
    pub moved: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Payload {
    Agent(Target),
    Moved(Moved),
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub to: String,
    pub body: String,
    pub data: Payload,
    pub sound: &'static str,
    pub priority: &'static str,
    pub channel_id: &'static str,
}

#[derive(Deserialize, Default)]
struct Answer {
    #[serde(default)]
    data: Vec<Ticket>,
}

#[derive(Deserialize, Default)]
struct Ticket {
    #[serde(default)]
    status: String,
    #[serde(default)]
    details: Option<Details>,
}

#[derive(Deserialize, Default)]
struct Details {
    #[serde(default)]
    error: Option<String>,
}

pub fn path(home: &Home) -> PathBuf {
    home.root().join(FILE)
}

pub fn phones(home: &Home) -> Vec<Phone> {
    fsutil::read_json_opt::<Vec<Phone>>(&path(home)).ok().flatten().unwrap_or_default()
}

fn keep(home: &Home, list: &[Phone]) -> io::Result<()> {
    if list.is_empty() {
        return fsutil::remove_file_if_exists(&path(home));
    }
    fsutil::write_json_atomic(&path(home), &list)
}

fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn valid_token(token: &str) -> bool {
    token.len() <= MAX_TOKEN
        && TOKEN_PREFIXES.iter().any(|prefix| {
            token.strip_prefix(prefix).and_then(|rest| rest.strip_suffix(TOKEN_END)).is_some_and(plain_id)
        })
}

pub fn registered(list: &[Phone], client: &str, push: Option<&Push>) -> Vec<Phone> {
    let target = push.filter(|push| valid_token(&push.token) && plain_id(&push.device));
    let mut next: Vec<Phone> = list
        .iter()
        .filter(|phone| phone.client != client && target.is_none_or(|push| phone.token != push.token))
        .cloned()
        .collect();
    if let Some(push) = target {
        next.push(Phone { client: client.to_string(), token: push.token.clone(), device: push.device.clone() });
    }
    let excess = next.len().saturating_sub(MAX_PHONES);
    next.drain(..excess);
    next
}

pub fn register(home: &Home, client: Option<&str>, push: Option<&Push>) {
    let Some(client) = client.filter(|client| plain_id(client)) else { return };
    let current = phones(home);
    let next = registered(&current, client, push);
    if next == current {
        return;
    }
    if let Err(error) = keep(home, &next) {
        bridge::diag(home, &format!("could not keep the notifications of a phone: {error}"));
    }
}

pub fn messages(phones: &[Phone], alerts: &[Alert]) -> Vec<Message> {
    phones
        .iter()
        .flat_map(|phone| {
            alerts.iter().map(move |alert| Message {
                to: phone.token.clone(),
                body: alert.message(),
                data: Payload::Agent(Target { device: phone.device.clone(), agent: alert.id }),
                sound: SOUND,
                priority: PRIORITY,
                channel_id: CHANNEL,
            })
        })
        .collect()
}

pub fn moved(phones: &[Phone], address: &str) -> Vec<Message> {
    phones
        .iter()
        .map(|phone| Message {
            to: phone.token.clone(),
            body: MOVED_BODY.to_string(),
            data: Payload::Moved(Moved { device: phone.device.clone(), moved: address.to_string() }),
            sound: SOUND,
            priority: PRIORITY,
            channel_id: CHANNEL,
        })
        .collect()
}

pub fn unregistered(batch: &[Message], answer: &[u8]) -> Vec<String> {
    let Ok(answer) = serde_json::from_slice::<Answer>(answer) else { return Vec::new() };
    batch
        .iter()
        .zip(answer.data)
        .filter(|(_, ticket)| {
            ticket.status == FAILED
                && ticket.details.as_ref().and_then(|details| details.error.as_deref()) == Some(UNREGISTERED)
        })
        .map(|(message, _)| message.to.clone())
        .collect()
}

fn post(url: &str, batch: &[Message]) -> io::Result<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .https_only(url.starts_with("https://"))
        .user_agent(format!("orkai/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let body = serde_json::to_vec(batch).map_err(io::Error::other)?;
    let mut response = agent
        .post(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|e| io::Error::other(format!("{url}: {e}")))?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_vec()
        .map_err(|e| io::Error::other(format!("{url}: {e}")))
}

fn send(home: &Home, url: &str, outgoing: &[Message]) {
    let mut gone = HashSet::new();
    for batch in outgoing.chunks(BATCH) {
        match post(url, batch) {
            Ok(answer) => gone.extend(unregistered(batch, &answer)),
            Err(error) => bridge::diag(home, &format!("could not notify the phones: {error}")),
        }
    }
    if gone.is_empty() {
        return;
    }
    let left: Vec<Phone> = phones(home).into_iter().filter(|phone| !gone.contains(&phone.token)).collect();
    if let Err(error) = keep(home, &left) {
        bridge::diag(home, &format!("could not forget the phones that left: {error}"));
    }
}

pub fn deliver_to(home: &Home, url: &str, alerts: &[Alert]) {
    send(home, url, &messages(&phones(home), alerts));
}

pub fn announce_move_to(home: &Home, url: &str, address: &str) {
    send(home, url, &moved(&phones(home), address));
}

pub fn announce_move(home: &Home, address: &str) {
    if phones(home).is_empty() {
        return;
    }
    let (home, address) = (home.clone(), address.to_string());
    let _ = thread::Builder::new()
        .name("orkai-push-moved".into())
        .spawn(move || announce_move_to(&home, EXPO_PUSH_URL, &address));
}

pub fn notify(home: &Home, alerts: &[Alert]) {
    if alerts.is_empty() || phones(home).is_empty() {
        return;
    }
    let (home, alerts) = (home.clone(), alerts.to_vec());
    let _ = thread::Builder::new().name("orkai-push".into()).spawn(move || deliver_to(&home, EXPO_PUSH_URL, &alerts));
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;
    use crate::model::{AgentKind, AgentState};
    use crate::util::now_ms;

    const TOKEN: &str = "ExponentPushToken[xxxxxxxxxxxxxxxxxxxxxx]";
    const OTHER: &str = "ExponentPushToken[yyyyyyyyyyyyyyyyyyyyyy]";

    fn home() -> Home {
        let dir = std::env::temp_dir().join(format!(
            "orkai-pwa-push-{}-{}-{}",
            std::process::id(),
            now_ms(),
            crate::util::to_hex(&crate::util::random_bytes::<4>().unwrap())
        ));
        let home = Home::at(&dir);
        home.ensure().unwrap();
        home
    }

    fn push(token: &str, device: &str) -> Push {
        Push { token: token.into(), device: device.into() }
    }

    fn phone(client: &str, token: &str, device: &str) -> Phone {
        Phone { client: client.into(), token: token.into(), device: device.into() }
    }

    fn alert(id: AgentId, name: &str, state: AgentState) -> Alert {
        Alert {
            id,
            name: name.into(),
            kind: AgentKind::Claude,
            state,
            session: "default".into(),
            cwd: "/work".into(),
            activity: String::new(),
        }
    }

    #[test]
    fn a_phone_that_allows_notifications_is_kept_once_with_its_latest_token() {
        let first = registered(&[], "c1", Some(&push(TOKEN, "d1")));
        assert_eq!(first, [phone("c1", TOKEN, "d1")]);
        let renewed = registered(&first, "c1", Some(&push(OTHER, "d1")));
        assert_eq!(renewed, [phone("c1", OTHER, "d1")], "a new token replaces the old one of the same app");
        let both = registered(&renewed, "c2", Some(&push(TOKEN, "d9")));
        assert_eq!(both, [phone("c1", OTHER, "d1"), phone("c2", TOKEN, "d9")]);
        let reinstalled = registered(&both, "c3", Some(&push(OTHER, "d1")));
        assert_eq!(
            reinstalled,
            [phone("c2", TOKEN, "d9"), phone("c3", OTHER, "d1")],
            "one token never gets two pushes"
        );
    }

    #[test]
    fn a_phone_that_turns_notifications_off_or_sends_junk_is_forgotten() {
        let list = [phone("c1", TOKEN, "d1"), phone("c2", OTHER, "d2")];
        assert_eq!(registered(&list, "c1", None), [phone("c2", OTHER, "d2")]);
        for junk in [
            push("not-a-token", "d1"),
            push("ExponentPushToken[]", "d1"),
            push("ExponentPushToken[a b]", "d1"),
            push(TOKEN, ""),
        ] {
            assert_eq!(registered(&list, "c1", Some(&junk)), [phone("c2", OTHER, "d2")], "{junk:?}");
        }
        let long = format!("ExponentPushToken[{}]", "a".repeat(MAX_TOKEN));
        assert_eq!(registered(&list, "c1", Some(&push(&long, "d1"))), [phone("c2", OTHER, "d2")]);
        assert_eq!(
            registered(&[], "c1", Some(&push("ExpoPushToken[abc-_9]", "d1"))),
            [phone("c1", "ExpoPushToken[abc-_9]", "d1")]
        );
    }

    #[test]
    fn the_list_of_phones_stays_bounded_dropping_the_oldest() {
        let mut list = Vec::new();
        for index in 0..MAX_PHONES + 3 {
            list = registered(&list, &format!("c{index}"), Some(&push(&format!("ExponentPushToken[t{index}]"), "d")));
        }
        assert_eq!(list.len(), MAX_PHONES);
        assert_eq!(list.first().unwrap().client, "c3");
        assert_eq!(list.last().unwrap().client, format!("c{}", MAX_PHONES + 2));
    }

    #[test]
    fn the_phones_are_kept_on_disk_for_the_desktop_and_the_file_goes_with_the_last_one() {
        let home = home();
        register(&home, Some("c1"), Some(&push(TOKEN, "d1")));
        assert_eq!(phones(&home), [phone("c1", TOKEN, "d1")]);
        register(&home, None, Some(&push(OTHER, "d2")));
        register(&home, Some("bad id"), Some(&push(OTHER, "d2")));
        assert_eq!(phones(&home), [phone("c1", TOKEN, "d1")], "a phone without a plain id is never kept");
        register(&home, Some("c1"), None);
        assert!(phones(&home).is_empty());
        assert!(!path(&home).exists());
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn every_alert_reaches_every_phone_with_the_agent_to_open_and_no_name_of_the_computer() {
        let list = [phone("c1", TOKEN, "d1"), phone("c2", OTHER, "d2")];
        let alerts = [alert(4, "fix-login", AgentState::Paused), alert(7, "docs", AgentState::Done)];
        let sent = messages(&list, &alerts);
        assert_eq!(sent.len(), 4);
        assert_eq!(
            serde_json::to_value(&sent[0]).unwrap(),
            serde_json::json!({
                "to": TOKEN,
                "body": "fix-login is waiting for you",
                "data": { "device": "d1", "agent": 4 },
                "sound": "default",
                "priority": "high",
                "channelId": "agents"
            })
        );
        assert_eq!(sent[1].body, "docs finished");
        assert_eq!(
            (sent[3].to.as_str(), &sent[3].data),
            (OTHER, &Payload::Agent(Target { device: "d2".into(), agent: 7 }))
        );
    }

    #[test]
    fn a_phone_the_push_service_no_longer_knows_is_named_and_other_errors_are_not() {
        let list = [phone("c1", TOKEN, "d1"), phone("c2", OTHER, "d2"), phone("c3", "ExponentPushToken[zz]", "d3")];
        let batch = messages(&list, &[alert(1, "a", AgentState::Done)]);
        let answer = br#"{"data":[{"status":"ok","id":"1"},{"status":"error","message":"gone","details":{"error":"DeviceNotRegistered"}},{"status":"error","details":{"error":"MessageRateExceeded"}}]}"#;
        assert_eq!(unregistered(&batch, answer), [OTHER]);
        assert!(unregistered(&batch, b"not json").is_empty());
        assert!(unregistered(&batch, br#"{"errors":[{"code":"PUSH_TOO_MANY_EXPERIENCE_IDS"}]}"#).is_empty());
    }

    fn push_service(answer: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/--/api/v2/push/send", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len()).unwrap();
            tx.send(String::from_utf8(request).unwrap()).unwrap();
        });
        (url, rx)
    }

    #[test]
    fn the_desktop_posts_the_alerts_to_the_push_service_and_forgets_uninstalled_phones() {
        let home = home();
        register(&home, Some("c1"), Some(&push(TOKEN, "d1")));
        register(&home, Some("c2"), Some(&push(OTHER, "d2")));
        let (url, rx) = push_service(
            r#"{"data":[{"status":"ok","id":"1"},{"status":"error","details":{"error":"DeviceNotRegistered"}}]}"#,
        );
        deliver_to(&home, &url, &[alert(3, "fix-login", AgentState::Done)]);
        let request = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(request.starts_with("POST /--/api/v2/push/send"));
        let body: serde_json::Value = serde_json::from_str(&request[request.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body[0]["to"], TOKEN);
        assert_eq!(body[1]["to"], OTHER);
        assert_eq!(body[0]["body"], "fix-login finished");
        assert_eq!(phones(&home), [phone("c1", TOKEN, "d1")], "the uninstalled phone gets no more pushes");
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn a_new_address_reaches_every_phone_with_the_computer_it_belongs_to_and_no_name() {
        let list = [phone("c1", TOKEN, "d1"), phone("c2", OTHER, "d2")];
        let sent = moved(&list, "https://new.trycloudflare.com");
        assert_eq!(
            serde_json::to_value(&sent).unwrap(),
            serde_json::json!([
                {
                    "to": TOKEN,
                    "body": MOVED_BODY,
                    "data": { "device": "d1", "moved": "https://new.trycloudflare.com" },
                    "sound": "default",
                    "priority": "high",
                    "channelId": "agents"
                },
                {
                    "to": OTHER,
                    "body": MOVED_BODY,
                    "data": { "device": "d2", "moved": "https://new.trycloudflare.com" },
                    "sound": "default",
                    "priority": "high",
                    "channelId": "agents"
                }
            ])
        );
        assert!(moved(&[], "https://new.trycloudflare.com").is_empty());
    }

    #[test]
    fn the_desktop_tells_the_phones_its_new_address_in_one_request() {
        let home = home();
        register(&home, Some("c1"), Some(&push(TOKEN, "d1")));
        register(&home, Some("c2"), Some(&push(OTHER, "d2")));
        let (url, rx) = push_service(r#"{"data":[{"status":"ok","id":"1"},{"status":"ok","id":"2"}]}"#);
        announce_move_to(&home, &url, "https://new.trycloudflare.com");
        let request = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let body: serde_json::Value = serde_json::from_str(&request[request.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body.as_array().map(Vec::len), Some(2), "one batch for every phone");
        assert_eq!(body[0]["data"], serde_json::json!({ "device": "d1", "moved": "https://new.trycloudflare.com" }));
        assert_eq!(body[1]["to"], OTHER);
        assert_eq!(phones(&home).len(), 2);
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn without_phones_a_new_address_is_not_announced() {
        let home = home();
        announce_move_to(&home, "http://127.0.0.1:9/never", "https://new.trycloudflare.com");
        announce_move(&home, "https://new.trycloudflare.com");
        assert!(!bridge::log_path(&home).exists(), "with no phone there is no request to fail");
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn without_phones_or_alerts_nothing_is_sent() {
        let home = home();
        deliver_to(&home, "http://127.0.0.1:9/never", &[alert(1, "a", AgentState::Done)]);
        assert!(!bridge::log_path(&home).exists(), "with no phone there is no request to fail");
        register(&home, Some("c1"), Some(&push(TOKEN, "d1")));
        deliver_to(&home, "http://127.0.0.1:9/never", &[]);
        assert!(!bridge::log_path(&home).exists(), "with no alert there is no request to fail");
        std::fs::remove_dir_all(home.root()).unwrap();
    }
}
