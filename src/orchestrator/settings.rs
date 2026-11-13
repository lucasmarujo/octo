use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::model::AgentKind;
use crate::models;
use crate::orchestrator::{ENGINES, Mode, ProjectMode};
use crate::paths::{Home, create_private_dir};
use crate::pwa::protocol::present;

pub const MAX_PROJECT_MODES: usize = 256;
pub const MAX_PROJECT_CHARS: usize = 1_024;
const FILE: &str = "settings.json";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub mode: Mode,
    pub projects: Vec<ProjectMode>,
    pub primary: Option<AgentKind>,
    pub secondary: Option<AgentKind>,
    pub primary_model: Option<String>,
    pub secondary_model: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChange {
    pub path: String,
    pub mode: Option<Mode>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectChange>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub primary: Option<Option<AgentKind>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub secondary: Option<Option<AgentKind>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub primary_model: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "present")]
    pub secondary_model: Option<Option<String>>,
}

pub fn file(home: &Home) -> PathBuf {
    home.orchestrator_dir().join(FILE)
}

fn engine(chosen: Option<AgentKind>) -> Result<Option<AgentKind>, String> {
    match chosen {
        Some(kind) if !ENGINES.contains(&kind) => Err(format!("{} cannot be Octo's engine", kind.label())),
        chosen => Ok(chosen),
    }
}

fn model(chosen: Option<String>) -> Result<Option<String>, String> {
    let chosen = chosen.map(|model| model.trim().to_string()).filter(|model| !model.is_empty());
    match chosen {
        Some(model) if !models::is_valid_id(&model) => Err("that model name is not valid".to_string()),
        chosen => Ok(chosen),
    }
}

impl Settings {
    pub fn load(home: &Home) -> Self {
        fsutil::read_json_opt(&file(home)).ok().flatten().unwrap_or_default()
    }

    pub fn save(&self, home: &Home) -> io::Result<()> {
        create_private_dir(&home.orchestrator_dir())?;
        fsutil::write_json_atomic(&file(home), self)
    }

    pub fn mode_for(&self, project: &str) -> Mode {
        self.projects.iter().find(|entry| entry.path == project).map_or(self.mode, |entry| entry.mode)
    }

    pub fn apply(&mut self, change: SettingsChange) -> Result<bool, String> {
        let mut next = self.clone();
        if let Some(enabled) = change.enabled {
            next.enabled = enabled;
        }
        if let Some(mode) = change.mode {
            next.mode = mode;
        }
        if let Some(project) = change.project {
            next.set_project(project)?;
        }
        if let Some(primary) = change.primary {
            next.primary = engine(primary)?;
        }
        if let Some(secondary) = change.secondary {
            next.secondary = engine(secondary)?;
        }
        if let Some(primary_model) = change.primary_model {
            next.primary_model = model(primary_model)?;
        }
        if let Some(secondary_model) = change.secondary_model {
            next.secondary_model = model(secondary_model)?;
        }
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }

    fn set_project(&mut self, change: ProjectChange) -> Result<(), String> {
        let path = change.path.trim();
        if path.is_empty() || path.chars().count() > MAX_PROJECT_CHARS {
            return Err("that project cannot have its own mode".to_string());
        }
        let at = self.projects.iter().position(|entry| entry.path == path);
        match (change.mode, at) {
            (Some(mode), Some(at)) => self.projects[at].mode = mode,
            (Some(_), None) if self.projects.len() >= MAX_PROJECT_MODES => {
                return Err(format!("only {MAX_PROJECT_MODES} projects can have their own mode"));
            }
            (Some(mode), None) => self.projects.push(ProjectMode { path: path.to_string(), mode }),
            (None, Some(at)) => {
                self.projects.remove(at);
            }
            (None, None) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;

    fn temp_home(tag: &str) -> (PathBuf, Home) {
        let dir = std::env::temp_dir().join(format!("orkai-octo-settings-{tag}-{}-{}", std::process::id(), now_ms()));
        (dir.clone(), Home::at(dir))
    }

    fn change(text: &str) -> SettingsChange {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn octo_starts_disabled_and_only_observing() {
        let (dir, home) = temp_home("default");
        let settings = Settings::load(&home);
        assert_eq!(settings, Settings::default());
        assert!(!settings.enabled);
        assert_eq!(settings.mode, Mode::Observe);
        assert_eq!((settings.primary, settings.secondary), (None, None));
        assert!(!dir.exists(), "reading the settings writes nothing");
    }

    #[test]
    fn settings_are_kept_between_runs_and_a_broken_or_older_file_falls_back_to_the_defaults() {
        let (dir, home) = temp_home("round-trip");
        let saved = Settings {
            enabled: true,
            mode: Mode::Moderate,
            projects: vec![ProjectMode { path: "dev/octo".into(), mode: Mode::Bypass }],
            primary: Some(AgentKind::Claude),
            secondary: Some(AgentKind::Codex),
            primary_model: Some("opus".into()),
            secondary_model: None,
        };
        saved.save(&home).unwrap();
        assert_eq!(Settings::load(&home), saved);
        let text = std::fs::read_to_string(file(&home)).unwrap();
        assert!(text.contains("\"primaryModel\": \"opus\""), "{text}");

        std::fs::write(file(&home), r#"{"enabled":true}"#).unwrap();
        assert_eq!(Settings::load(&home), Settings { enabled: true, ..Settings::default() });
        std::fs::write(file(&home), "{ not json").unwrap();
        assert_eq!(Settings::load(&home), Settings::default());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_change_touches_only_the_fields_it_names() {
        let mut settings = Settings::default();
        assert_eq!(settings.apply(change("{}")), Ok(false));
        assert_eq!(settings.apply(change(r#"{"enabled":true,"mode":"moderate","primary":"claude"}"#)), Ok(true));
        assert_eq!(settings.apply(change(r#"{"primaryModel":" opus ","secondary":"codex"}"#)), Ok(true));
        assert_eq!(
            settings,
            Settings {
                enabled: true,
                mode: Mode::Moderate,
                primary: Some(AgentKind::Claude),
                secondary: Some(AgentKind::Codex),
                primary_model: Some("opus".into()),
                ..Settings::default()
            }
        );
        assert_eq!(settings.apply(change(r#"{"mode":"moderate"}"#)), Ok(false), "the same value changes nothing");
        assert_eq!(settings.apply(change(r#"{"primary":null,"primaryModel":null}"#)), Ok(true));
        assert_eq!((settings.primary, settings.primary_model.clone()), (None, None));
        assert_eq!(settings.secondary, Some(AgentKind::Codex), "a field left out stays as it was");
        assert_eq!(settings.apply(change(r#"{"secondaryModel":"  "}"#)), Ok(false), "a blank model is no model");
    }

    #[test]
    fn an_absent_field_and_a_null_field_are_told_apart() {
        assert_eq!(change("{}").primary, None);
        assert_eq!(change(r#"{"primary":null}"#).primary, Some(None));
        assert_eq!(change(r#"{"primary":"codex"}"#).primary, Some(Some(AgentKind::Codex)));
        assert_eq!(change(r#"{"secondaryModel":null}"#).secondary_model, Some(None));
        assert_eq!(change(r#"{"secondaryModel":"gpt-6"}"#).secondary_model, Some(Some("gpt-6".into())));
        assert_eq!(change(r#"{"project":{"path":"p","mode":null}}"#).project.unwrap().mode, None);
        let sent = serde_json::to_string(&SettingsChange { primary: Some(None), ..SettingsChange::default() }).unwrap();
        assert_eq!(sent, r#"{"primary":null}"#);
        assert!(serde_json::from_str::<SettingsChange>(r#"{"primary":"notepad"}"#).is_err());
    }

    #[test]
    fn only_an_engine_with_a_valid_model_name_is_accepted_and_a_refused_change_changes_nothing() {
        let mut settings = Settings { mode: Mode::Moderate, ..Settings::default() };
        let refused = settings.apply(change(r#"{"mode":"bypass","primary":"kimi"}"#)).unwrap_err();
        assert!(refused.contains("kimi"), "{refused}");
        assert_eq!(settings.mode, Mode::Moderate, "no part of a refused change is kept");
        assert!(settings.apply(change(r#"{"secondary":"terminal"}"#)).is_err());
        assert!(settings.apply(change(r#"{"primaryModel":"--dangerous"}"#)).is_err());
        assert!(settings.apply(change(r#"{"secondaryModel":"a b"}"#)).is_err());
        assert_eq!(settings, Settings { mode: Mode::Moderate, ..Settings::default() });
    }

    #[test]
    fn a_project_can_have_its_own_mode_and_give_it_back() {
        let mut settings = Settings { mode: Mode::Observe, ..Settings::default() };
        assert_eq!(settings.apply(change(r#"{"project":{"path":" dev/octo ","mode":"bypass"}}"#)), Ok(true));
        assert_eq!(settings.mode_for("dev/octo"), Mode::Bypass);
        assert_eq!(settings.mode_for("dev/other"), Mode::Observe, "other projects keep the default mode");
        assert_eq!(settings.apply(change(r#"{"project":{"path":"dev/octo","mode":"moderate"}}"#)), Ok(true));
        assert_eq!(settings.projects, [ProjectMode { path: "dev/octo".into(), mode: Mode::Moderate }]);
        assert_eq!(settings.apply(change(r#"{"project":{"path":"dev/octo","mode":null}}"#)), Ok(true));
        assert!(settings.projects.is_empty());
        assert_eq!(settings.apply(change(r#"{"project":{"path":"dev/octo","mode":null}}"#)), Ok(false));
        assert!(settings.apply(change(r#"{"project":{"path":"  ","mode":"bypass"}}"#)).is_err());
    }

    #[test]
    fn the_projects_with_their_own_mode_are_bounded() {
        let mut settings = Settings::default();
        for index in 0..MAX_PROJECT_MODES {
            let project = ProjectChange { path: format!("p{index}"), mode: Some(Mode::Moderate) };
            settings.apply(SettingsChange { project: Some(project), ..SettingsChange::default() }).unwrap();
        }
        let extra = ProjectChange { path: "one-more".into(), mode: Some(Mode::Moderate) };
        assert!(settings.apply(SettingsChange { project: Some(extra), ..SettingsChange::default() }).is_err());
        assert_eq!(settings.projects.len(), MAX_PROJECT_MODES);
        let known = ProjectChange { path: "p0".into(), mode: Some(Mode::Bypass) };
        assert_eq!(settings.apply(SettingsChange { project: Some(known), ..SettingsChange::default() }), Ok(true));
    }
}
