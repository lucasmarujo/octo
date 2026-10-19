use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use orkai::usage::{
    AUTH_FILE, CLAUDE_CREDENTIALS_FILE, CODEX_AUTH_FILE, KIMI_CONFIG_FILE, Limits, Problem, Window, fetch_claude,
    fetch_codex, fetch_grok, fetch_kilo, fetch_kimi, fetch_opencode,
};
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
    std::fs::create_dir_all(dir.join(file).parent().unwrap()).unwrap();
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
    assert_eq!(
        limits,
        Limits {
            session: window(55.0, 1_792_108_800_278),
            weekly: window(8.0, 1_792_216_800_278),
            ..Limits::default()
        }
    );
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
    assert_eq!(
        limits,
        Limits {
            session: window(12.0, 1_792_108_800_000),
            weekly: window(40.0, 1_792_216_800_000),
            ..Limits::default()
        }
    );
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

fn base_of(server: &Server) -> String {
    server.url.trim_end_matches("/usage").to_string()
}

#[test]
fn opencode_go_usage_is_read_with_the_go_key_in_one_request() {
    let server = serve(
        "200 OK",
        r#"{"usage":{"rolling":{"status":"ok","percent":34,"resetsAt":"2026-10-16T00:00:00.000Z"},"weekly":{"status":"ok","percent":61,"resetsAt":"2026-10-17T06:00:00.000Z"},"monthly":{"status":"ok","percent":12,"resetsAt":"2026-11-01T00:00:00.000Z"}}}"#,
    );
    let data = home_with(
        AUTH_FILE,
        r#"{"opencode-go":{"type":"api","key":"sk-go-1"},"opencode":{"type":"api","key":"sk-zen"}}"#,
    );
    let limits = fetch_opencode(Some(&data.0), &server.url).unwrap();
    assert_eq!(limits.session, window(34.0, 1_792_108_800_000));
    assert_eq!(limits.weekly, window(61.0, 1_792_216_800_000));
    assert!(limits.monthly.is_some());
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(header(&requests[0], "authorization"), Some("Bearer sk-go-1"));

    let rejected =
        serve("401 Unauthorized", r#"{"type":"error","error":{"type":"AuthError","message":"Unauthorized"}}"#);
    assert_eq!(fetch_opencode(Some(&data.0), &rejected.url), Err(Problem::Expired));
}

#[test]
fn kilo_balance_and_kilo_pass_are_read_with_the_kilo_login() {
    let server = serve("200 OK", r#"{"balance":7.5}"#);
    let data = home_with(
        AUTH_FILE,
        r#"{"kilo":{"type":"oauth","access":"kilo-token","refresh":"kilo-token","expires":4102444800000}}"#,
    );
    let limits = fetch_kilo(Some(&data.0), &base_of(&server)).unwrap();
    assert_eq!(limits.credits, Some(7.5));
    assert_eq!(limits.monthly, None, "an answer that is not a kilo pass state shows no pass");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0][0].starts_with("GET /api/profile/balance "), "{:?}", requests[0]);
    assert!(requests[1][0].starts_with("GET /api/trpc/kiloPass.getState?"), "{:?}", requests[1]);
    for request in &requests {
        assert_eq!(header(request, "authorization"), Some("Bearer kilo-token"));
        assert_eq!(header(request, "x-kilocode-organizationid"), None);
    }
    assert_eq!(header(&requests[1], "x-kilocode-editorname"), Some("Kilo CLI"));

    let team = serve("200 OK", r#"{"balance":"30"}"#);
    let org = home_with(AUTH_FILE, r#"{"kilo":{"type":"oauth","access":"t","accountId":"org-9"}}"#);
    assert_eq!(fetch_kilo(Some(&org.0), &base_of(&team)).unwrap().credits, Some(30.0));
    let requests = team.requests();
    assert_eq!(requests.len(), 1, "team balances have no personal kilo pass");
    assert_eq!(header(&requests[0], "x-kilocode-organizationid"), Some("org-9"));
}

#[test]
fn grok_billing_is_read_with_the_grok_login_headers() {
    let server = serve(
        "200 OK",
        r#"{"config":{"creditUsagePercent":42,"currentPeriod":{"type":"WEEKLY","start":"2026-10-09T00:00:00Z","end":"2026-10-16T00:00:00Z"}}}"#,
    );
    let auth = home_with(
        AUTH_FILE,
        r#"{"https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828":{"key":"grok-jwt","auth_mode":"oidc","user_id":"u-7","expires_at":"2999-01-01T00:00:00Z"},"xai::api_key":{"key":"xai-key","auth_mode":"api_key"}}"#,
    );
    let limits = fetch_grok(Some(&auth.0.join(AUTH_FILE)), &server.url, now_ms()).unwrap();
    assert_eq!(limits.weekly, window(42.0, 1_792_108_800_000));
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(header(request, "authorization"), Some("Bearer grok-jwt"));
    assert_eq!(header(request, "x-xai-token-auth"), Some("xai-grok-cli"));
    assert_eq!(header(request, "x-userid"), Some("u-7"));
    assert_eq!(header(request, "x-grok-client-mode"), Some("interactive"));
}

#[test]
fn kimi_usage_is_read_from_the_coding_endpoint_with_the_oauth_token() {
    let server = serve(
        "200 OK",
        r#"{"goods_version":2,"usages":{"limit_5h":{"used_ratio":0.25,"reset_time":"2026-10-16T00:00:00Z"},"limit_7d":{"used_ratio":0.5,"reset_time":"2026-10-17T06:00:00Z"}}}"#,
    );
    let expires = now_ms() / 1_000 + 3_600;
    let home = home_with(
        "credentials/kimi-code.json",
        &format!(
            r#"{{"access_token":"kimi-token","refresh_token":"r","expires_at":{expires},"scope":"kimi-code","token_type":"Bearer","expires_in":3600}}"#
        ),
    );
    let base = format!("{}/coding/v1/", base_of(&server));
    let limits = fetch_kimi(Some(&home.0), &base, now_ms()).unwrap();
    assert_eq!(limits.session, window(25.0, 1_792_108_800_000));
    assert_eq!(limits.weekly, window(50.0, 1_792_216_800_000));
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0][0].starts_with("GET /coding/v1/usages "), "{:?}", requests[0]);
    assert_eq!(header(&requests[0], "authorization"), Some("Bearer kimi-token"));

    std::fs::write(
        home.0.join(KIMI_CONFIG_FILE),
        "[providers.\"managed:kimi-code\"]\nbase_url = \"http://203.0.113.1/coding/v1\"\noauth = { storage = \"file\", key = \"oauth/kimi-code\" }\n",
    )
    .unwrap();
    fetch_kimi(Some(&home.0), &base, now_ms()).unwrap();
    assert_eq!(server.requests().len(), 2, "the token is never sent to a plain http base_url from config.toml");
}
