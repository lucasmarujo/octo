use std::io;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::paths::Home;
use crate::pwa::http;
use crate::util::{random_bytes, to_hex};

pub const BASE: &str = "https://octo-relay.marujo.dev";
const FILE: &str = "pwa-relay.json";
const COMPUTERS: &str = "/v1/computers/";
const TIMEOUT: Duration = Duration::from_secs(15);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ANSWER_BYTES: u64 = 4 * 1024;
const ID_BYTES: usize = 16;
const TOKEN_BYTES: usize = 32;
const TAKEN: u16 = 403;
const SECURE: &str = "https://";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

#[derive(Serialize)]
struct Tunnel<'a> {
    url: &'a str,
}

#[derive(Deserialize)]
struct Published {
    address: String,
}

pub fn path(home: &Home) -> PathBuf {
    home.root().join(FILE)
}

fn hex_of(text: &str, bytes: usize) -> bool {
    text.len() == bytes * 2 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn load(home: &Home) -> Option<Identity> {
    fsutil::read_json_opt::<Identity>(&path(home))
        .ok()
        .flatten()
        .filter(|identity| hex_of(&identity.id, ID_BYTES) && hex_of(&identity.token, TOKEN_BYTES))
}

pub fn known(home: &Home) -> bool {
    load(home).is_some()
}

pub fn address(home: &Home) -> Option<String> {
    load(home).and_then(|identity| identity.address)
}

pub fn identity(home: &Home) -> io::Result<Identity> {
    if let Some(identity) = load(home) {
        return Ok(identity);
    }
    let identity = Identity {
        id: to_hex(&random_bytes::<ID_BYTES>()?),
        token: to_hex(&random_bytes::<TOKEN_BYTES>()?),
        address: None,
    };
    fsutil::write_json_atomic(&path(home), &identity)?;
    Ok(identity)
}

pub fn forget(home: &Home) -> io::Result<()> {
    fsutil::remove_file_if_exists(&path(home))
}

fn agent(url: &str, timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(url.starts_with(SECURE))
        .user_agent(format!("orkai/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn fixed_address(identity: &Identity, answer: &[u8]) -> io::Result<String> {
    let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidData, format!("fixed address: {message}"));
    let published: Published = serde_json::from_slice(answer).map_err(|_| invalid("the answer is not json"))?;
    let host = published.address.strip_prefix(SECURE).ok_or_else(|| invalid("the address is not https"))?;
    let plain = host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
    if !plain || !host.starts_with(&format!("{}.", identity.id)) {
        return Err(invalid("the address is not the one of this computer"));
    }
    Ok(published.address)
}

pub fn publish_to(base: &str, identity: &Identity, tunnel: &str) -> io::Result<String> {
    let url = format!("{base}{COMPUTERS}{}", identity.id);
    let body = serde_json::to_vec(&Tunnel { url: tunnel }).map_err(io::Error::other)?;
    let sent = agent(&url, TIMEOUT)
        .put(&url)
        .header("Authorization", format!("Bearer {}", identity.token))
        .header("Content-Type", "application/json")
        .send(body);
    let mut response = match sent {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(TAKEN)) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the fixed address of this computer was taken; a new one replaces it",
            ));
        }
        Err(error) => return Err(io::Error::other(format!("the fixed address was not updated: {error}"))),
    };
    let answer = response
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_vec()
        .map_err(|error| io::Error::other(format!("the fixed address was not updated: {error}")))?;
    fixed_address(identity, &answer)
}

pub fn publish(home: &Home, tunnel: &str) -> io::Result<String> {
    let mut identity = identity(home)?;
    let address = publish_to(BASE, &identity, tunnel)?;
    if identity.address.as_deref() != Some(address.as_str()) {
        identity.address = Some(address.clone());
        fsutil::write_json_atomic(&path(home), &identity)?;
    }
    Ok(address)
}

pub fn answers(address: &str) -> io::Result<()> {
    let url = format!("{address}{}", http::HEALTH_PATH);
    let silent =
        |error: String| io::Error::other(format!("the fixed address does not reach this computer yet: {error}"));
    let mut response = agent(&url, ANSWER_TIMEOUT).get(&url).call().map_err(|error| silent(error.to_string()))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_vec()
        .map_err(|error| silent(error.to_string()))?;
    if body != http::HEALTH_BODY.as_bytes() {
        return Err(silent("something else answered".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;
    use crate::util::now_ms;

    const TUNNEL: &str = "https://quiet-river-sound-lamp.trycloudflare.com";

    fn home() -> Home {
        let dir = std::env::temp_dir().join(format!(
            "orkai-pwa-relay-{}-{}-{}",
            std::process::id(),
            now_ms(),
            to_hex(&random_bytes::<4>().unwrap())
        ));
        let home = Home::at(&dir);
        home.ensure().unwrap();
        home
    }

    fn service(answers: Vec<String>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let read = stream.read(&mut chunk).unwrap();
                    request.extend_from_slice(&chunk[..read]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    let Some(end) = text.find("\r\n\r\n") else { continue };
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                stream.write_all(answer.as_bytes()).unwrap();
                tx.send(String::from_utf8(request).unwrap()).unwrap();
            }
        });
        (base, rx)
    }

    fn ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn refused(status: &str) -> String {
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    #[test]
    fn a_computer_keeps_one_random_identity_across_restarts() {
        let home = home();
        assert!(!known(&home));
        let first = identity(&home).unwrap();
        assert!(hex_of(&first.id, ID_BYTES) && hex_of(&first.token, TOKEN_BYTES));
        assert_eq!(first.address, None);
        assert!(known(&home));
        assert_eq!(identity(&home).unwrap(), first, "the same computer is the same address forever");
        let other = self::home();
        assert_ne!(identity(&other).unwrap().id, first.id, "two computers never share an address");
        forget(&home).unwrap();
        assert!(!known(&home));
        assert_ne!(identity(&home).unwrap().id, first.id);
        std::fs::remove_dir_all(home.root()).unwrap();
        std::fs::remove_dir_all(other.root()).unwrap();
    }

    #[test]
    fn a_damaged_identity_is_replaced_instead_of_used() {
        let home = home();
        for junk in [r#"{"id":"abc","token":"def"}"#, r#"{"id":"../../etc","token":""}"#, "not json"] {
            std::fs::write(path(&home), junk).unwrap();
            assert!(!known(&home), "{junk}");
        }
        let fresh = identity(&home).unwrap();
        assert!(hex_of(&fresh.id, ID_BYTES));
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn the_tunnel_is_told_to_the_service_with_the_token_of_this_computer() {
        let home = home();
        let me = identity(&home).unwrap();
        let address = format!("https://{}.octo.marujo.dev", me.id);
        let (base, rx) = service(vec![ok(&format!(r#"{{"address":"{address}"}}"#))]);
        assert_eq!(publish_to(&base, &me, TUNNEL).unwrap(), address);
        let request = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(request.starts_with(&format!("PUT /v1/computers/{} HTTP/1.1\r\n", me.id)), "{request}");
        assert!(request.to_ascii_lowercase().contains(&format!("authorization: bearer {}\r\n", me.token)));
        let body: serde_json::Value = serde_json::from_str(&request[request.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body, serde_json::json!({ "url": TUNNEL }));
        std::fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn an_address_that_is_not_of_this_computer_is_never_shown() {
        let me = Identity { id: "0123456789abcdef0123456789abcdef".into(), token: "a".repeat(64), address: None };
        for answer in [
            r#"{"address":"https://evil.example.com"}"#,
            r#"{"address":"http://0123456789abcdef0123456789abcdef.octo.marujo.dev"}"#,
            r#"{"address":"https://0123456789abcdef0123456789abcdef.octo.marujo.dev/#k=x"}"#,
            r#"{"address":"https://0123456789abcdef0123456789abcdefevil.example.com"}"#,
            r#"{"address":"https://fedcba9876543210fedcba9876543210.octo.marujo.dev"}"#,
            r#"{"other":1}"#,
            "not json",
        ] {
            let err = fixed_address(&me, answer.as_bytes()).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{answer}");
        }
        assert_eq!(
            fixed_address(&me, br#"{"address":"https://0123456789abcdef0123456789abcdef.octo.marujo.dev"}"#).unwrap(),
            "https://0123456789abcdef0123456789abcdef.octo.marujo.dev"
        );
    }

    #[test]
    fn a_taken_address_is_told_apart_from_a_service_that_is_down() {
        let me = Identity { id: "0123456789abcdef0123456789abcdef".into(), token: "a".repeat(64), address: None };
        let (base, _rx) = service(vec![refused("403 Forbidden"), refused("503 Service Unavailable")]);
        assert_eq!(publish_to(&base, &me, TUNNEL).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(publish_to(&base, &me, TUNNEL).unwrap_err().kind(), io::ErrorKind::Other);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let gone = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(publish_to(&gone, &me, TUNNEL).unwrap_err().kind(), io::ErrorKind::Other);
    }

    #[test]
    fn the_address_is_ready_only_when_this_computer_answers_through_it() {
        let health = |body: &str| {
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        };
        let (bridge, _seen) = service(vec![health(http::HEALTH_BODY)]);
        let moved = format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {bridge}/health\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let (fixed, asked) = service(vec![moved, health("someone else")]);
        answers(&fixed).unwrap();
        assert!(asked.recv_timeout(Duration::from_secs(10)).unwrap().starts_with("GET /health HTTP/1.1\r\n"));
        assert!(answers(&fixed).is_err(), "another server at the address is not this computer");
    }
}
