use std::process::Command;
use std::thread;
use std::time::Duration;

use orkai::config::Config;
use orkai::launch::{command_output, resolve_program, wrap_for_platform};
use orkai::model::{AgentId, AgentKind};
use serde::Serialize;
use tauri::State;

use crate::install::{self, OS, Os};
use crate::link::Link;

pub type Version = (u32, u32, u32);

const VERSION_ARG: &str = "--version";
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const LATEST_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_LATEST_BYTES: u64 = 1024 * 1024;
const NPM_REGISTRY: &str = "https://registry.npmjs.org";
const ANTIGRAVITY_MANIFESTS: &str = "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Latest {
    Text(String),
    Json(String),
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUpdate {
    pub kind: AgentKind,
    pub installed: String,
    pub latest: String,
}

fn npm(package: &str) -> Latest {
    Latest::Json(format!("{NPM_REGISTRY}/{package}/latest"))
}

fn antigravity_platform(os: Os, arch: &str) -> Option<String> {
    let system = match os {
        Os::Windows => "windows",
        Os::Mac => "darwin",
        Os::Linux => "linux",
    };
    let cpu = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };
    Some(format!("{system}_{cpu}"))
}

pub fn latest_source(kind: AgentKind, os: Os, arch: &str) -> Option<Latest> {
    match kind {
        AgentKind::Claude => Some(Latest::Text("https://downloads.claude.ai/claude-code-releases/latest".into())),
        AgentKind::Codex => Some(npm("@openai/codex")),
        AgentKind::Kilo => Some(npm("@kilocode/cli")),
        AgentKind::Opencode => Some(npm("@opencode/cli")),
        AgentKind::Antigravity => antigravity_platform(os, arch)
            .map(|platform| Latest::Json(format!("{ANTIGRAVITY_MANIFESTS}/{platform}.json"))),
        AgentKind::Grok => Some(Latest::Text("https://x.ai/cli/stable".into())),
        AgentKind::Qwen => Some(npm("@qwen-code/qwen-code")),
        AgentKind::Kimi => Some(Latest::Text("https://code.kimi.com/kimi-code/latest".into())),
        AgentKind::Pi => Some(Latest::Json("https://pi.dev/api/installer/releases/latest".into())),
        AgentKind::Terminal => None,
    }
}

fn version_token(token: &str) -> Option<Version> {
    let core = token.trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let version = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    parts.next().is_none().then_some(version)
}

pub fn find_version(text: &str) -> Option<Version> {
    text.split(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')' | '"' | '\'' | ':')).find_map(version_token)
}

pub fn shown(version: Version) -> String {
    format!("{}.{}.{}", version.0, version.1, version.2)
}

pub fn installed_version(command: &str) -> Option<Version> {
    let program = resolve_program(command).ok()?;
    let (program, args) = wrap_for_platform(program, vec![VERSION_ARG.to_string()]).ok()?;
    find_version(&command_output(Command::new(program).args(args), VERSION_TIMEOUT)?)
}

fn fetch(url: &str) -> Option<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(LATEST_TIMEOUT))
        .https_only(url.starts_with("https://"))
        .user_agent(format!("octo/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    agent.get(url).call().ok()?.body_mut().with_config().limit(MAX_LATEST_BYTES).read_to_vec().ok()
}

pub fn latest_version(source: &Latest) -> Option<Version> {
    match source {
        Latest::Text(url) => find_version(String::from_utf8(fetch(url)?).ok()?.trim()),
        Latest::Json(url) => {
            let value: serde_json::Value = serde_json::from_slice(&fetch(url)?).ok()?;
            version_token(value.get("version")?.as_str()?)
        }
    }
}

pub fn newer(kind: AgentKind, installed: Version, latest: Version) -> Option<AgentUpdate> {
    (latest > installed).then(|| AgentUpdate { kind, installed: shown(installed), latest: shown(latest) })
}

pub fn check(
    config: &Config,
    kinds: &[AgentKind],
    installed: impl Fn(&str) -> Option<Version> + Sync,
    latest: impl Fn(AgentKind) -> Option<Version> + Sync,
) -> Vec<AgentUpdate> {
    let agents: Vec<(AgentKind, &str)> = kinds
        .iter()
        .filter(|kind| kind.is_ai())
        .filter_map(|kind| config.agent(*kind).map(|(command, _)| (*kind, command)))
        .collect();
    let (installed, latest) = (&installed, &latest);
    thread::scope(|scope| {
        let checks: Vec<_> = agents
            .into_iter()
            .map(|(kind, command)| scope.spawn(move || newer(kind, installed(command)?, latest(kind)?)))
            .collect();
        checks.into_iter().filter_map(|check| check.join().ok().flatten()).collect()
    })
}

#[tauri::command(async)]
pub fn agent_updates(link: State<'_, Link>) -> Vec<AgentUpdate> {
    let hub = link.hub();
    let config = hub.launcher().config().clone();
    check(&config, &hub.kinds(), installed_version, |kind| {
        latest_version(&latest_source(kind, OS, std::env::consts::ARCH)?)
    })
}

#[tauri::command(async)]
pub fn agent_version(link: State<'_, Link>, kind: AgentKind) -> Option<String> {
    let config = link.hub().launcher().config().clone();
    let (command, _) = config.agent(kind)?;
    installed_version(command).map(shown)
}

#[tauri::command(async)]
pub fn update_agent(link: State<'_, Link>, kind: AgentKind, session: String) -> Result<AgentId, String> {
    install::start_update(link.hub(), kind, &session)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Instant;

    use super::*;

    const OSES: [Os; 3] = [Os::Windows, Os::Mac, Os::Linux];
    const ARCHES: [&str; 2] = ["x86_64", "aarch64"];

    fn ai_kinds() -> impl Iterator<Item = AgentKind> {
        AgentKind::ALL.into_iter().filter(|kind| kind.is_ai())
    }

    fn serve(answers: Vec<(&'static str, &'static str)>) -> String {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            for (status, body) in answers {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let reply =
                    format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        address
    }

    #[test]
    fn the_version_is_found_in_whatever_each_agent_prints() {
        assert_eq!(find_version("2.1.319 (Claude Code)"), Some((2, 1, 319)));
        assert_eq!(find_version("codex-cli 0.167.0\n"), Some((0, 167, 0)));
        assert_eq!(find_version("1.5.1\r\n"), Some((1, 5, 1)));
        assert_eq!(find_version("v1.4.0"), Some((1, 4, 0)));
        assert_eq!(find_version("kimi, version 2.5.2"), Some((2, 5, 2)));
        assert_eq!(find_version("opencode 0.0.0-beta-17236"), Some((0, 0, 0)), "a pre-release counts as its core");
        assert_eq!(find_version("\"1.5.0\""), Some((1, 5, 0)));
        assert_eq!(find_version("Claude Code"), None);
        assert_eq!(find_version("1.2"), None);
        assert_eq!(find_version("10.0.19045.1"), None, "a build number of four parts is not a version");
        assert_eq!(find_version(""), None);
        assert_eq!(find_version("<html>Service unavailable</html>"), None);
    }

    #[test]
    fn only_a_strictly_newer_version_is_an_update_and_parts_compare_as_numbers() {
        let update = newer(AgentKind::Pi, (1, 4, 0), (1, 5, 0)).unwrap();
        assert_eq!(update, AgentUpdate { kind: AgentKind::Pi, installed: "1.4.0".into(), latest: "1.5.0".into() });
        assert!(newer(AgentKind::Codex, (0, 9, 0), (0, 10, 0)).is_some(), "0.10 is newer than 0.9");
        assert!(newer(AgentKind::Codex, (0, 167, 0), (0, 167, 0)).is_none(), "up to date");
        assert!(
            newer(AgentKind::Codex, (0, 168, 0), (0, 167, 0)).is_none(),
            "ahead of the release, as a preview build"
        );
        let json = serde_json::to_value(update).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "pi", "installed": "1.4.0", "latest": "1.5.0" }));
    }

    #[test]
    fn every_ai_agent_asks_the_same_place_its_installer_asks_and_only_over_https() {
        for os in OSES {
            for arch in ARCHES {
                for kind in ai_kinds() {
                    let source = latest_source(kind, os, arch).unwrap_or_else(|| panic!("{kind:?} on {os:?} {arch}"));
                    let (Latest::Text(url) | Latest::Json(url)) = source;
                    assert!(url.starts_with("https://"), "{url}");
                }
                assert_eq!(latest_source(AgentKind::Terminal, os, arch), None, "a terminal has no version");
            }
        }
        let json = |url: &str| Some(Latest::Json(url.into()));
        assert_eq!(latest_source(AgentKind::Pi, OS, "x86_64"), json("https://pi.dev/api/installer/releases/latest"));
        assert_eq!(
            latest_source(AgentKind::Codex, OS, "x86_64"),
            json("https://registry.npmjs.org/@openai/codex/latest")
        );
        assert_eq!(
            latest_source(AgentKind::Claude, OS, "x86_64"),
            Some(Latest::Text("https://downloads.claude.ai/claude-code-releases/latest".into()))
        );
        let antigravity = |os, arch| match latest_source(AgentKind::Antigravity, os, arch) {
            Some(Latest::Json(url)) => url.rsplit('/').next().unwrap().to_string(),
            other => panic!("{other:?}"),
        };
        assert_eq!(antigravity(Os::Windows, "x86_64"), "windows_amd64.json", "the name its install.ps1 uses");
        assert_eq!(antigravity(Os::Mac, "aarch64"), "darwin_arm64.json");
        assert_eq!(antigravity(Os::Linux, "x86_64"), "linux_amd64.json");
        assert_eq!(latest_source(AgentKind::Antigravity, Os::Linux, "riscv64"), None, "no build to compare with");
    }

    #[test]
    fn the_latest_version_is_read_from_a_text_or_a_json_answer_and_anything_else_is_ignored() {
        let base = serve(vec![
            ("200 OK", "2.1.319\n"),
            ("200 OK", r#"{"name":"@earendil-works/pi-coding-agent","version":"1.5.0","bin":{"pi":"dist/cli.js"}}"#),
            ("200 OK", "<html>Service unavailable</html>"),
            ("404 Not Found", "1.0.0"),
            ("200 OK", r#"{"version":7}"#),
        ]);
        assert_eq!(latest_version(&Latest::Text(format!("{base}/latest"))), Some((2, 1, 319)));
        assert_eq!(latest_version(&Latest::Json(format!("{base}/pi"))), Some((1, 5, 0)));
        assert_eq!(latest_version(&Latest::Text(format!("{base}/down"))), None, "an error page is no version");
        assert_eq!(latest_version(&Latest::Text(format!("{base}/gone"))), None, "a failed request is no version");
        assert_eq!(latest_version(&Latest::Json(format!("{base}/odd"))), None);
        let started = Instant::now();
        assert_eq!(latest_version(&Latest::Text("http://127.0.0.1:9/latest".into())), None);
        assert!(
            started.elapsed() < LATEST_TIMEOUT + Duration::from_secs(2),
            "an unreachable server never hangs the check"
        );
    }

    #[test]
    fn the_installed_version_comes_from_the_command_octo_runs() {
        let dir =
            std::env::temp_dir().join(format!("octo-agent-version-{}-{}", std::process::id(), orkai::util::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let program: PathBuf = if cfg!(windows) {
            let path = dir.join("fake-agent.cmd");
            std::fs::write(&path, "@echo off\r\necho fake-agent 1.4.0\r\n").unwrap();
            path
        } else {
            let path = dir.join("fake-agent");
            std::fs::write(&path, "#!/bin/sh\necho 'fake-agent 1.4.0'\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            path
        };
        assert_eq!(installed_version(&program.display().to_string()), Some((1, 4, 0)));
        assert_eq!(installed_version(&dir.join("missing-agent").display().to_string()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_check_lists_only_installed_ai_agents_behind_their_release_and_asks_them_all_at_once() {
        let kinds = [AgentKind::Claude, AgentKind::Codex, AgentKind::Pi, AgentKind::Kimi, AgentKind::Terminal];
        let delay = Duration::from_millis(300);
        let installed = |command: &str| {
            thread::sleep(delay);
            match command {
                "claude" => Some((2, 1, 319)),
                "codex" => Some((0, 160, 0)),
                "pi" => Some((1, 4, 0)),
                _ => None,
            }
        };
        let latest = |kind: AgentKind| {
            thread::sleep(delay);
            match kind {
                AgentKind::Claude => Some((2, 1, 319)),
                AgentKind::Pi => Some((1, 5, 0)),
                AgentKind::Kimi => Some((9, 9, 9)),
                AgentKind::Terminal => panic!("a terminal is never checked"),
                _ => None,
            }
        };
        let started = Instant::now();
        let updates = check(&Config::default(), &kinds, installed, latest);
        assert_eq!(
            updates,
            vec![AgentUpdate { kind: AgentKind::Pi, installed: "1.4.0".into(), latest: "1.5.0".into() }]
        );
        assert!(started.elapsed() < delay * 4, "every agent is checked at the same time: {:?}", started.elapsed());
        let custom = Config { pi_command: "my-pi".into(), ..Config::default() };
        let seen = Mutex::new(Vec::new());
        let reading = |command: &str| {
            seen.lock().unwrap().push(command.to_string());
            None
        };
        assert!(check(&custom, &[AgentKind::Pi], reading, |_| None).is_empty());
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["my-pi".to_string()],
            "the command set in Settings, not the default name"
        );
    }

    #[test]
    fn the_window_can_only_name_a_kind_and_a_session_to_update() {
        let code = include_str!("agent_updates.rs");
        assert!(
            code.contains(
                "pub fn update_agent(link: State<'_, Link>, kind: AgentKind, session: String) -> Result<AgentId, String>"
            ),
            "no text that could run"
        );
        assert!(code.contains("pub fn agent_version(link: State<'_, Link>, kind: AgentKind) -> Option<String>"));
        let runs = code.split("#[cfg(test)]").next().unwrap();
        assert!(!runs.contains("env::var"), "the environment chooses no address");
    }
}
