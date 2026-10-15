use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use orkai::usage::{CLAUDE_CREDENTIALS_FILE, CODEX_AUTH_FILE, Limits, Problem, Window, fetch_claude, fetch_codex};
use orkai::util::now_ms;

const CLAUDE_USAGE: &str = r#"{"five_hour":{"utilization":55.0,"resets_at":"2026-10-16T00:00:00.278740+00:00"},"seven_day":{"utilization":8.0,"resets_at":"2026-10-17T06:00:00.278758+00:00"},"seven_day_opus":null}"#;
const CODEX_USAGE: &str = r#"{"plan_type":"plus","rate_limit":{"allowed":true,"limit_reached":false,"primary_window":{"used_percent":12,"limit_window_seconds":18000,"reset_after_seconds":100,"reset_at":1792108800},"secondary_window":{"used_percent":40,"limit_window_seconds":604800,"reset_after_seconds":100,"reset_at":1792216800}}}"#;
const UNREACHABLE: &str = "http://127.0.0.1:9/usage";
static COUNTER: AtomicU64 = AtomicU64::new(0);

type Requests = Arc<Mutex<Vec<Vec<String>>>>;

struct Server {
    url: String,
    requests: Requests,
}

impl Server {
    fn requests(&self) -> Vec<Vec<String>> {
        self.requests.lock().unwrap().clone()
    }
}

fn serve(status: &'static str, body: &'static str) -> Server {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://{}/usage", listener.local_addr().unwrap());
    let requests: Requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut lines = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                    break;
                }
                lines.push(line.trim_end().to_string());
            }
            seen.lock().unwrap().push(lines);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Server { url, requests }
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn home_with(file: &str, contents: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!(
        "orkai-usage-{}-{}-{}",
        std::process::id(),
        now_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(file), contents).unwrap();
    Dir(dir)
}

fn claude_home(token: &str, expires_at: u64) -> Dir {
    let credentials = format!(
        r#"{{"claudeAiOauth":{{"accessToken":"{token}","refreshToken":"refresh","expiresAt":{expires_at},"scopes":["user:profile"],"subscriptionType":"max"}}}}"#
    );
    home_with(CLAUDE_CREDENTIALS_FILE, &credentials)
}

fn header<'a>(request: &'a [String], name: &str) -> Option<&'a str> {
    request.iter().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn window(used_percent: f64, resets_at_ms: u64) -> Option<Window> {
    Some(Window { used_percent, resets_at_ms: Some(resets_at_ms) })
}

#[test]
fn claude_usage_is_read_with_the_oauth_token_in_one_request() {
    let server = serve("200 OK", CLAUDE_USAGE);
    let now = now_ms();
    let home = claude_home("sk-ant-oat01-test", now + 3_600_000);
    let limits = fetch_claude(Some(&home.0), &server.url, now).unwrap();
    assert_eq!(limits, Limits { session: window(55.0, 1_792_108_800_278), weekly: window(8.0, 1_792_216_800_278) });
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(request[0].starts_with("GET /usage "), "{request:?}");
    assert_eq!(header(request, "authorization"), Some("Bearer sk-ant-oat01-test"));
    assert_eq!(header(request, "anthropic-beta"), Some("oauth-2025-04-20"));
    assert!(header(request, "user-agent").is_some_and(|ua| ua.starts_with("orkai/")));
}

#[test]
fn claude_rejected_or_expired_logins_are_reported() {
    let server = serve("401 Unauthorized", r#"{"error":"invalid token"}"#);
    let now = now_ms();
    let home = claude_home("stale", now + 3_600_000);
    assert_eq!(fetch_claude(Some(&home.0), &server.url, now), Err(Problem::Expired));

    let expired = claude_home("expired", now - 1);
    assert_eq!(fetch_claude(Some(&expired.0), UNREACHABLE, now), Err(Problem::Expired));
}

#[test]
fn claude_without_login_does_not_call_the_api() {
    let server = serve("200 OK", CLAUDE_USAGE);
    let now = now_ms();
    let api_key_only = home_with(CLAUDE_CREDENTIALS_FILE, r#"{"mcpOAuth":{}}"#);
    let missing = home_with("other.json", "{}");
    if !cfg!(target_os = "macos") {
        assert_eq!(fetch_claude(Some(&missing.0), &server.url, now), Err(Problem::SignedOut));
        assert_eq!(fetch_claude(None, &server.url, now), Err(Problem::SignedOut));
    }
    assert_eq!(fetch_claude(Some(&api_key_only.0), &server.url, now), Err(Problem::SignedOut));
    assert!(server.requests().is_empty());
}

#[test]
fn claude_failures_are_unavailable() {
    let server = serve("500 Internal Server Error", "{}");
    let now = now_ms();
    let home = claude_home("token", now + 3_600_000);
    assert_eq!(fetch_claude(Some(&home.0), &server.url, now), Err(Problem::Unavailable));
    assert_eq!(fetch_claude(Some(&home.0), UNREACHABLE, now), Err(Problem::Unavailable));
    let garbage = serve("200 OK", "<html>blocked</html>");
    assert_eq!(fetch_claude(Some(&home.0), &garbage.url, now), Err(Problem::Unavailable));
}

#[test]
fn codex_usage_is_read_with_the_chatgpt_account_in_one_request() {
    let server = serve("200 OK", CODEX_USAGE);
    let auth = r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"header.payload.sig","access_token":"codex-access","refresh_token":"refresh","account_id":"acct-123"},"last_refresh":"2026-10-15T12:00:00Z"}"#;
    let home = home_with(CODEX_AUTH_FILE, auth);
    let limits = fetch_codex(Some(&home.0), &server.url).unwrap();
    assert_eq!(limits, Limits { session: window(12.0, 1_792_108_800_000), weekly: window(40.0, 1_792_216_800_000) });
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(header(&requests[0], "authorization"), Some("Bearer codex-access"));
    assert_eq!(header(&requests[0], "chatgpt-account-id"), Some("acct-123"));
}

#[test]
fn codex_api_keys_and_missing_logins_do_not_call_the_api() {
    let server = serve("200 OK", CODEX_USAGE);
    let api_key = home_with(CODEX_AUTH_FILE, r#"{"OPENAI_API_KEY":"sk-test","tokens":null}"#);
    assert_eq!(fetch_codex(Some(&api_key.0), &server.url), Err(Problem::ApiKey));
    let empty = home_with(CODEX_AUTH_FILE, r#"{"OPENAI_API_KEY":null}"#);
    assert_eq!(fetch_codex(Some(&empty.0), &server.url), Err(Problem::SignedOut));
    let missing = home_with("config.toml", "");
    assert_eq!(fetch_codex(Some(&missing.0), &server.url), Err(Problem::SignedOut));
    assert_eq!(fetch_codex(None, &server.url), Err(Problem::SignedOut));
    assert!(server.requests().is_empty());

    let rejected = serve("403 Forbidden", "{}");
    let auth = home_with(CODEX_AUTH_FILE, r#"{"tokens":{"access_token":"old","account_id":null}}"#);
    assert_eq!(fetch_codex(Some(&auth.0), &rejected.url), Err(Problem::Expired));
    assert_eq!(header(&rejected.requests()[0], "chatgpt-account-id"), None);
}
