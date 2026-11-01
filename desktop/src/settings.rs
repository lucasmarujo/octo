use std::path::{Path, PathBuf};

use orkai::config::Config;
use orkai::fsutil;
use orkai::hooks::parse_version;
use orkai::launch::{default_shell, installed_kinds};
use orkai::model::AgentKind;
use orkai::paths::user_home;
use orkai::pwa::hub::Hub;
use orkai::sound::{Chime, Player};
use orkai::tui::format::display_path;
use orkai::update::{self, Source};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::link::Link;
use crate::updates;

pub const MIN_PORT: u16 = 1024;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub config: Config,
    pub path: String,
    pub agents: Vec<AgentStatus>,
    pub default_shell: String,
    pub shell_rule: &'static str,
}

pub const SHELL_RULE: &str = if cfg!(windows) { "pwsh, then powershell" } else { "your $SHELL, then bash" };

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub kind: AgentKind,
    pub installed: bool,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub latest: String,
    pub newer: bool,
    pub ready: bool,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sound {
    Waiting,
    Done,
}

pub fn read(hub: &Hub) -> Settings {
    let config = hub.launcher().config().clone();
    let installed = installed_kinds(&config);
    let agents = AgentKind::ALL
        .into_iter()
        .filter(|kind| kind.is_ai())
        .map(|kind| AgentStatus { kind, installed: installed.contains(&kind) })
        .collect();
    let default_shell = default_shell(&Config { shell: None, ..config.clone() }).0;
    let local = std::env::var_os("LOCALAPPDATA").filter(|_| cfg!(windows)).map(PathBuf::from);
    let path = shown(&hub.store().home().config_file(), local.as_deref(), user_home().as_deref());
    Settings { config, path, agents, default_shell, shell_rule: SHELL_RULE }
}

fn shown(path: &Path, local_app_data: Option<&Path>, home: Option<&Path>) -> String {
    match local_app_data.and_then(|local| path.strip_prefix(local).ok()) {
        Some(rest) => format!("%LOCALAPPDATA%{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => display_path(path, home),
    }
}

pub fn tidy(mut config: Config) -> Result<Config, String> {
    if config.pwa_port < MIN_PORT {
        return Err(format!("The local port has to be between {MIN_PORT} and 65535."));
    }
    config.shell = config.shell.map(|shell| shell.trim().to_string()).filter(|shell| !shell.is_empty());
    config.notify_command =
        config.notify_command.map(|command| command.trim().to_string()).filter(|command| !command.is_empty());
    let defaults = Config::default();
    for kind in AgentKind::ALL.into_iter().filter(|kind| kind.is_ai()) {
        let command = command_mut(&mut config, kind);
        *command = command.trim().to_string();
        if command.is_empty() {
            *command = defaults.agent(kind).map(|(default, _)| default.to_string()).unwrap_or_default();
        }
    }
    Ok(config)
}

fn command_mut(config: &mut Config, kind: AgentKind) -> &mut String {
    match kind {
        AgentKind::Claude => &mut config.claude_command,
        AgentKind::Codex => &mut config.codex_command,
        AgentKind::Kilo => &mut config.kilo_command,
        AgentKind::Opencode => &mut config.opencode_command,
        AgentKind::Antigravity => &mut config.agy_command,
        AgentKind::Grok => &mut config.grok_command,
        AgentKind::Qwen => &mut config.qwen_command,
        AgentKind::Kimi => &mut config.kimi_command,
        AgentKind::Pi => &mut config.pi_command,
        AgentKind::Terminal => unreachable!("a terminal has no command of its own"),
    }
}

pub fn save(hub: &Hub, config: Config) -> Result<Settings, String> {
    let config = tidy(config)?;
    fsutil::write_json_atomic(&hub.store().home().config_file(), &config).map_err(|err| err.to_string())?;
    hub.reconfigure(config).map_err(|err| err.to_string())?;
    Ok(read(hub))
}

pub fn check(source: &Source) -> Result<UpdateCheck, String> {
    let manifest = source.manifest().map_err(|err| err.to_string())?;
    let newer = parse_version(&manifest.version)
        .zip(parse_version(&update::VERSION))
        .is_some_and(|(latest, running)| latest > running);
    Ok(UpdateCheck { latest: manifest.version, newer, ready: false })
}

#[tauri::command]
pub fn settings(link: State<'_, Link>) -> Settings {
    read(link.hub())
}

#[tauri::command]
pub fn save_settings(link: State<'_, Link>, config: Config) -> Result<Settings, String> {
    save(link.hub(), config)
}

#[tauri::command]
pub fn play_chime(link: State<'_, Link>, sound: Sound) -> bool {
    let chime = match sound {
        Sound::Waiting => Chime::Waiting,
        Sound::Done => Chime::Done,
    };
    Player::new(link.hub().store().home()).is_some_and(|player| player.play(chime))
}

#[tauri::command]
pub async fn check_update(app: tauri::AppHandle) -> Result<UpdateCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut checked = check(&Source::from_env())?;
        if checked.newer && updates::updates_itself() {
            checked.ready = updates::install_and_announce(&app)?.is_some();
        }
        Ok(checked)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use orkai::launch::Launcher;
    use orkai::paths::Home;
    use orkai::store::Store;

    use super::*;

    struct Fixture {
        dir: PathBuf,
        hub: Arc<Hub>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!(
            "orkai-desktop-settings-{name}-{}-{}",
            std::process::id(),
            orkai::util::now_ms()
        ));
        let home = Home::at(dir.join("home"));
        let store = Store::open(home.clone()).unwrap();
        let config = Config::load_or_init(&home).unwrap();
        let launcher = Launcher::new(store, config, &std::env::current_exe().unwrap()).unwrap();
        Fixture { hub: Hub::new(launcher, "testhost".into()), dir }
    }

    #[test]
    fn the_settings_list_every_ai_agent_with_whether_it_is_installed() {
        let fx = fixture("read");
        let settings = read(&fx.hub);
        let kinds: Vec<AgentKind> = settings.agents.iter().map(|agent| agent.kind).collect();
        let expected: Vec<AgentKind> = AgentKind::ALL.into_iter().filter(|kind| kind.is_ai()).collect();
        assert_eq!(kinds, expected);
        for agent in &settings.agents {
            assert_eq!(agent.installed, fx.hub.kinds().contains(&agent.kind), "{:?}", agent.kind);
        }
        assert!(settings.path.ends_with("config.json"), "{}", settings.path);
        assert!(!settings.default_shell.is_empty());
    }

    #[test]
    fn saved_settings_land_in_the_file_and_in_the_hub_right_away() {
        let fx = fixture("save");
        let exe = std::env::current_exe().unwrap().display().to_string();
        let config = Config {
            claude_command: format!("  {exe} "),
            codex_command: String::new(),
            claude_args: vec!["--model".into(), "opus".into()],
            shell: Some("   ".into()),
            notify_sound: false,
            ..Config::default()
        };
        let settings = save(&fx.hub, config).unwrap();
        assert_eq!(settings.config.claude_command, exe, "commands are trimmed");
        assert_eq!(settings.config.codex_command, "codex", "an empty command goes back to the default");
        assert_eq!(settings.config.shell, None, "a blank shell means the default one");
        let written: Config =
            serde_json::from_slice(&std::fs::read(fx.hub.store().home().config_file()).unwrap()).unwrap();
        assert_eq!(written, settings.config);
        assert_eq!(fx.hub.launcher().config(), &settings.config);
        assert!(fx.hub.kinds().contains(&AgentKind::Claude), "claude now points at a program that exists");
        assert!(settings.agents.iter().any(|agent| agent.kind == AgentKind::Claude && agent.installed));
    }

    #[test]
    fn a_port_below_the_reserved_range_is_refused_and_nothing_changes() {
        let fx = fixture("port");
        let before = std::fs::read(fx.hub.store().home().config_file()).unwrap();
        let err = save(&fx.hub, Config { pwa_port: 80, ..Config::default() }).unwrap_err();
        assert!(err.contains("1024"), "{err}");
        assert_eq!(std::fs::read(fx.hub.store().home().config_file()).unwrap(), before);
        assert_eq!(fx.hub.launcher().config().pwa_port, Config::default().pwa_port);
    }

    #[test]
    fn the_config_file_is_named_the_way_the_system_names_its_folder() {
        let local = Path::new("C:/Users/ana/AppData/Local");
        let file = local.join("orkai").join("config.json");
        let text = shown(&file, Some(local), None);
        assert!(text.starts_with("%LOCALAPPDATA%"), "{text}");
        assert!(text.ends_with("config.json") && text.contains("orkai"), "{text}");
        let home = Path::new("/home/ana");
        assert_eq!(shown(&home.join(".orkai/config.json"), None, Some(home)), "~/.orkai/config.json");
    }

    #[test]
    fn an_unreachable_release_server_is_reported_instead_of_hanging() {
        let err = check(&Source::at("http://127.0.0.1:9")).unwrap_err();
        assert!(!err.is_empty());
    }
}
