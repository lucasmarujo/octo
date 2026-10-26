use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use orkai::update::{Asset, MANIFEST_NAME, Manifest, Outcome, Source, apply, sha256_hex};
use orkai::util::now_ms;

const TARGET: &str = "test-target";
const OLD_COMMIT: &str = "1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NEW_COMMIT: &str = "2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OLD_VERSION: &str = "0.1.0";
const NEW_VERSION: &str = "0.2.0";
const NEW_BINARY: &[u8] = b"brand new orkai binary";

struct Server {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

fn serve(files: HashMap<String, Vec<u8>>) -> Server {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base_url = format!("http://{}/releases/download/latest", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() || header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            let path = request_line.split_whitespace().nth(1).unwrap_or_default().to_string();
            let name = path.rsplit('/').next().unwrap_or_default().to_string();
            seen.lock().unwrap().push(name.clone());
            let response = match files.get(&name) {
                Some(body) => {
                    let mut out =
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                            .into_bytes();
                    out.extend_from_slice(body);
                    out
                }
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            };
            let _ = stream.write_all(&response);
        }
    });
    Server { base_url, requests }
}

fn manifest(binary: &[u8]) -> Manifest {
    let mut assets = BTreeMap::new();
    assets.insert(
        TARGET.to_string(),
        Asset {
            name: format!("orkai-{TARGET}"),
            sha256: sha256_hex(binary),
            size: u64::try_from(binary.len()).unwrap(),
        },
    );
    Manifest { commit: NEW_COMMIT.into(), version: NEW_VERSION.into(), built_at: "now".into(), assets }
}

fn release(manifest: &Manifest, served_binary: &[u8]) -> Server {
    let mut files = HashMap::new();
    files.insert(MANIFEST_NAME.to_string(), serde_json::to_vec(manifest).unwrap());
    files.insert(format!("orkai-{TARGET}"), served_binary.to_vec());
    serve(files)
}

struct Install {
    dir: PathBuf,
    exe: PathBuf,
}

impl Drop for Install {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn installed(tag: &str) -> Install {
    let dir = std::env::temp_dir().join(format!("orkai-update-e2e-{tag}-{}-{}", std::process::id(), now_ms()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(if cfg!(windows) { "orkai.exe" } else { "orkai" });
    std::fs::write(&exe, b"old orkai binary").unwrap();
    Install { dir, exe }
}

fn dir_entries(install: &Install) -> usize {
    std::fs::read_dir(&install.dir).unwrap().count()
}

#[test]
fn outdated_build_downloads_verifies_and_replaces_itself() {
    let install = installed("update");
    let server = release(&manifest(NEW_BINARY), NEW_BINARY);
    let source = Source::at(&server.base_url);
    let fetched = source.manifest().unwrap();
    assert_eq!(fetched.commit, NEW_COMMIT);
    let outcome = apply(&source, &fetched, &install.exe, Some(OLD_COMMIT), OLD_VERSION, TARGET).unwrap();
    assert_eq!(outcome, Outcome::Updated { to: NEW_COMMIT.into() });
    assert_eq!(std::fs::read(&install.exe).unwrap(), NEW_BINARY);
    assert_eq!(dir_entries(&install), 1);
    assert_eq!(*server.requests.lock().unwrap(), vec![MANIFEST_NAME.to_string(), format!("orkai-{TARGET}")]);
}

#[test]
fn current_build_downloads_nothing() {
    let install = installed("current");
    let server = release(&manifest(NEW_BINARY), NEW_BINARY);
    let source = Source::at(&server.base_url);
    let fetched = source.manifest().unwrap();
    let outcome = apply(&source, &fetched, &install.exe, Some(NEW_COMMIT), NEW_VERSION, TARGET).unwrap();
    assert_eq!(outcome, Outcome::UpToDate);
    assert_eq!(std::fs::read(&install.exe).unwrap(), b"old orkai binary");
    assert_eq!(*server.requests.lock().unwrap(), vec![MANIFEST_NAME.to_string()]);
}

#[test]
fn newer_push_of_the_released_version_downloads_nothing() {
    let install = installed("same-version");
    let server = release(&manifest(NEW_BINARY), NEW_BINARY);
    let source = Source::at(&server.base_url);
    let fetched = source.manifest().unwrap();
    let outcome = apply(&source, &fetched, &install.exe, Some(OLD_COMMIT), NEW_VERSION, TARGET).unwrap();
    assert_eq!(outcome, Outcome::UpToDate);
    assert_eq!(std::fs::read(&install.exe).unwrap(), b"old orkai binary");
    assert_eq!(*server.requests.lock().unwrap(), vec![MANIFEST_NAME.to_string()]);
}

#[test]
fn tampered_binary_is_rejected_and_nothing_changes() {
    let install = installed("tampered");
    let tampered = b"brand new orkai binarX";
    let server = release(&manifest(NEW_BINARY), tampered);
    let source = Source::at(&server.base_url);
    let fetched = source.manifest().unwrap();
    let err = apply(&source, &fetched, &install.exe, Some(OLD_COMMIT), OLD_VERSION, TARGET).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");
    assert_eq!(std::fs::read(&install.exe).unwrap(), b"old orkai binary");
    assert_eq!(dir_entries(&install), 1);
}

#[test]
fn truncated_binary_is_rejected() {
    let install = installed("truncated");
    let server = release(&manifest(NEW_BINARY), &NEW_BINARY[..5]);
    let source = Source::at(&server.base_url);
    let fetched = source.manifest().unwrap();
    let err = apply(&source, &fetched, &install.exe, Some(OLD_COMMIT), OLD_VERSION, TARGET).unwrap_err();
    assert!(err.to_string().contains("expected"), "{err}");
    assert_eq!(std::fs::read(&install.exe).unwrap(), b"old orkai binary");
}

#[test]
fn platform_without_binary_is_reported() {
    let install = installed("noasset");
    let server = release(&manifest(NEW_BINARY), NEW_BINARY);
    let source = Source::at(&server.base_url);
    let fetched = source.manifest().unwrap();
    let outcome = apply(&source, &fetched, &install.exe, Some(OLD_COMMIT), OLD_VERSION, "other-target").unwrap();
    assert_eq!(outcome, Outcome::NoAsset);
}

#[test]
fn missing_release_is_an_error() {
    let server = serve(HashMap::new());
    assert!(Source::at(&server.base_url).manifest().is_err());
}

#[test]
fn unreachable_server_fails_fast() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let started = std::time::Instant::now();
    assert!(Source::at(&base).manifest().is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
