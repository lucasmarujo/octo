use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::launch;
use crate::model::AgentKind;
use crate::util::{now_ms, rfc3339_to_ms};

pub const PROVIDERS: [AgentKind; 2] = [AgentKind::Claude, AgentKind::Codex];
pub const REFRESH_EVERY: Duration = Duration::from_secs(120);
pub const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
pub const CLAUDE_CREDENTIALS_FILE: &str = ".credentials.json";
pub const CODEX_AUTH_FILE: &str = "auth.json";
const CLAUDE_OAUTH_BETA: &str = "oauth-2025-04-20";
#[cfg(target_os = "macos")]
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: u64 = 512 * 1024;
const SESSION_WINDOW_MAX_SECS: i64 = 24 * 3_600;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub used_percent: f64,
    pub resets_at_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limits {
    pub session: Option<Window>,
    pub weekly: Option<Window>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    SignedOut,
    Expired,
    ApiKey,
    Unavailable,
}

impl Problem {
    pub fn label(self) -> &'static str {
        match self {
            Problem::SignedOut => "not signed in",
            Problem::Expired => "login expired",
            Problem::ApiKey => "api key · no plan limits",
            Problem::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub limits: Option<Limits>,
    pub problem: Option<Problem>,
    pub updated_at_ms: Option<u64>,
}

impl Usage {
    pub fn apply(&mut self, result: Result<Limits, Problem>, now: u64) {
        match result {
            Ok(limits) => {
                self.limits = Some(limits);
                self.problem = None;
                self.updated_at_ms = Some(now);
            }
            Err(Problem::Unavailable) => self.problem = Some(Problem::Unavailable),
            Err(problem) => {
                self.limits = None;
                self.problem = Some(problem);
            }
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClaudeCredentials {
    claude_ai_oauth: Option<ClaudeOauth>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClaudeOauth {
    access_token: String,
    expires_at: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeUsage {
    five_hour: Option<ClaudeWindow>,
    seven_day: Option<ClaudeWindow>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexAuth {
    #[serde(rename = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,
    tokens: Option<CodexTokens>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexTokens {
    access_token: String,
    account_id: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexUsage {
    rate_limit: Option<CodexRateLimit>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexRateLimit {
    primary_window: Option<CodexWindow>,
    secondary_window: Option<CodexWindow>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexWindow {
    used_percent: f64,
    limit_window_seconds: i64,
    reset_at: i64,
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Problem> {
    serde_json::from_slice(bytes).map_err(|_| Problem::Unavailable)
}

fn get(url: &str, headers: &[(&str, String)]) -> Result<Vec<u8>, Problem> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .https_only(url.starts_with("https://"))
        .user_agent(format!("orkai/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut request = agent.get(url).header("Accept", "application/json");
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    match request.call() {
        Ok(mut response) => {
            response.body_mut().with_config().limit(MAX_RESPONSE_BYTES).read_to_vec().map_err(|_| Problem::Unavailable)
        }
        Err(ureq::Error::StatusCode(401 | 403)) => Err(Problem::Expired),
        Err(_) => Err(Problem::Unavailable),
    }
}

pub fn fetch(kind: AgentKind) -> Result<Limits, Problem> {
    match kind {
        AgentKind::Claude => fetch_claude(launch::claude_home().as_deref(), CLAUDE_USAGE_URL, now_ms()),
        AgentKind::Codex => fetch_codex(launch::codex_home().as_deref(), CODEX_USAGE_URL),
        AgentKind::Terminal => Err(Problem::Unavailable),
    }
}

#[cfg(target_os = "macos")]
fn keychain_credentials() -> Option<Vec<u8>> {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", CLAUDE_KEYCHAIN_SERVICE, "-w"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

#[cfg(not(target_os = "macos"))]
fn keychain_credentials() -> Option<Vec<u8>> {
    None
}

pub fn fetch_claude(claude_home: Option<&Path>, url: &str, now: u64) -> Result<Limits, Problem> {
    let bytes = claude_home
        .and_then(|home| fs::read(home.join(CLAUDE_CREDENTIALS_FILE)).ok())
        .or_else(keychain_credentials)
        .ok_or(Problem::SignedOut)?;
    let credentials: ClaudeCredentials = parse(&bytes)?;
    let oauth = credentials.claude_ai_oauth.filter(|o| !o.access_token.is_empty()).ok_or(Problem::SignedOut)?;
    if oauth.expires_at.is_some_and(|at| at <= now) {
        return Err(Problem::Expired);
    }
    let headers = [
        ("Authorization", format!("Bearer {}", oauth.access_token)),
        ("anthropic-beta", CLAUDE_OAUTH_BETA.to_string()),
    ];
    claude_limits(&get(url, &headers)?)
}

pub fn fetch_codex(codex_home: Option<&Path>, url: &str) -> Result<Limits, Problem> {
    let bytes = codex_home.and_then(|home| fs::read(home.join(CODEX_AUTH_FILE)).ok()).ok_or(Problem::SignedOut)?;
    let auth: CodexAuth = parse(&bytes)?;
    let Some(tokens) = auth.tokens.filter(|t| !t.access_token.is_empty()) else {
        let api_key = auth.openai_api_key.is_some_and(|key| !key.trim().is_empty());
        return Err(if api_key { Problem::ApiKey } else { Problem::SignedOut });
    };
    let mut headers = vec![("Authorization", format!("Bearer {}", tokens.access_token))];
    if let Some(account) = tokens.account_id.filter(|a| !a.is_empty()) {
        headers.push(("ChatGPT-Account-Id", account));
    }
    codex_limits(&get(url, &headers)?)
}

fn claude_window(window: Option<ClaudeWindow>) -> Option<Window> {
    let window = window?;
    Some(Window {
        used_percent: window.utilization?,
        resets_at_ms: window.resets_at.as_deref().and_then(rfc3339_to_ms),
    })
}

pub fn claude_limits(body: &[u8]) -> Result<Limits, Problem> {
    let usage: ClaudeUsage = parse(body)?;
    Ok(Limits { session: claude_window(usage.five_hour), weekly: claude_window(usage.seven_day) })
}

pub fn codex_limits(body: &[u8]) -> Result<Limits, Problem> {
    let usage: CodexUsage = parse(body)?;
    let mut limits = Limits::default();
    let Some(rate_limit) = usage.rate_limit else { return Ok(limits) };
    for (window, primary) in [(rate_limit.primary_window, true), (rate_limit.secondary_window, false)] {
        let Some(window) = window else { continue };
        let session = match window.limit_window_seconds {
            seconds if seconds > 0 => seconds <= SESSION_WINDOW_MAX_SECS,
            _ => primary,
        };
        let slot = if session { &mut limits.session } else { &mut limits.weekly };
        if slot.is_none() {
            *slot = Some(Window {
                used_percent: window.used_percent,
                resets_at_ms: u64::try_from(window.reset_at).ok().filter(|at| *at > 0).map(|at| at * 1_000),
            });
        }
    }
    Ok(limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_BODY: &str = r#"{"five_hour":{"utilization":55.0,"resets_at":"2026-10-16T00:00:00.278740+00:00","limit_dollars":null},"seven_day":{"utilization":8.0,"resets_at":"2026-10-17T06:00:00.278758+00:00"},"seven_day_opus":null,"extra_usage":{"is_enabled":false}}"#;

    fn window(used_percent: f64, resets_at_ms: Option<u64>) -> Option<Window> {
        Some(Window { used_percent, resets_at_ms })
    }

    #[test]
    fn claude_usage_reads_five_hour_and_weekly_windows() {
        let limits = claude_limits(CLAUDE_BODY.as_bytes()).unwrap();
        assert_eq!(limits.session, window(55.0, Some(1_792_108_800_278)));
        assert_eq!(limits.weekly, window(8.0, Some(1_792_216_800_278)));
    }

    #[test]
    fn claude_windows_without_utilization_are_missing() {
        let limits = claude_limits(br#"{"five_hour":null,"seven_day":{"utilization":null,"resets_at":null}}"#).unwrap();
        assert_eq!(limits, Limits::default());
        let limits = claude_limits(br#"{"five_hour":{"utilization":0.0,"resets_at":null}}"#).unwrap();
        assert_eq!(limits.session, window(0.0, None));
        assert_eq!(claude_limits(b"<html>"), Err(Problem::Unavailable));
    }

    #[test]
    fn codex_windows_are_classified_by_their_length() {
        let body = br#"{"plan_type":"plus","rate_limit":{"allowed":true,"limit_reached":false,
            "primary_window":{"used_percent":12,"limit_window_seconds":18000,"reset_after_seconds":60,"reset_at":1792108800},
            "secondary_window":{"used_percent":40,"limit_window_seconds":604800,"reset_after_seconds":60,"reset_at":1792216800}}}"#;
        let limits = codex_limits(body).unwrap();
        assert_eq!(limits.session, window(12.0, Some(1_792_108_800_000)));
        assert_eq!(limits.weekly, window(40.0, Some(1_792_216_800_000)));

        let weekly_only = br#"{"rate_limit":{"primary_window":{"used_percent":7,"limit_window_seconds":604800,"reset_at":0},"secondary_window":null}}"#;
        let limits = codex_limits(weekly_only).unwrap();
        assert_eq!(limits.session, None);
        assert_eq!(limits.weekly, window(7.0, None));
    }

    #[test]
    fn codex_windows_without_length_fall_back_to_their_position() {
        let body = br#"{"rate_limit":{"primary_window":{"used_percent":1,"reset_at":1792108800},"secondary_window":{"used_percent":2,"reset_at":1792216800}}}"#;
        let limits = codex_limits(body).unwrap();
        assert_eq!(limits.session.unwrap().used_percent, 1.0);
        assert_eq!(limits.weekly.unwrap().used_percent, 2.0);
        assert_eq!(codex_limits(br#"{"plan_type":"free","rate_limit":null}"#).unwrap(), Limits::default());
    }

    #[test]
    fn transient_failures_keep_the_last_limits_and_others_clear_them() {
        let limits = Limits { session: window(10.0, None), weekly: None };
        let mut usage = Usage::default();
        usage.apply(Ok(limits), 1_000);
        assert_eq!(usage, Usage { limits: Some(limits), problem: None, updated_at_ms: Some(1_000) });
        usage.apply(Err(Problem::Unavailable), 2_000);
        assert_eq!(
            usage,
            Usage { limits: Some(limits), problem: Some(Problem::Unavailable), updated_at_ms: Some(1_000) }
        );
        usage.apply(Ok(limits), 3_000);
        assert_eq!(usage.problem, None);
        usage.apply(Err(Problem::Expired), 4_000);
        assert_eq!(usage.limits, None);
        assert_eq!(usage.problem, Some(Problem::Expired));
    }

    #[test]
    fn terminals_have_no_usage() {
        assert_eq!(fetch(AgentKind::Terminal), Err(Problem::Unavailable));
        assert!(!PROVIDERS.contains(&AgentKind::Terminal));
    }
}
