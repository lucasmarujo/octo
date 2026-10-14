use std::io;

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::paths::Home;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub shell: Option<String>,
    pub shell_args: Vec<String>,
    pub claude_command: String,
    pub claude_args: Vec<String>,
    pub codex_command: String,
    pub codex_args: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shell: None,
            shell_args: Vec::new(),
            claude_command: "claude".to_string(),
            claude_args: Vec::new(),
            codex_command: "codex".to_string(),
            codex_args: Vec::new(),
        }
    }
}

impl Config {
    pub fn load_or_init(home: &Home) -> io::Result<Self> {
        let path = home.config_file();
        match fsutil::read_json_opt::<Config>(&path)? {
            Some(config) => Ok(config),
            None => {
                let config = Config::default();
                fsutil::write_json_atomic(&path, &config)?;
                Ok(config)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_uses_defaults_for_missing_fields() {
        let config: Config = serde_json::from_str(r#"{"claude_args":["--model","opus"]}"#).unwrap();
        assert_eq!(config.claude_command, "claude");
        assert_eq!(config.claude_args, vec!["--model", "opus"]);
        assert_eq!(config.codex_command, "codex");
        assert!(config.shell.is_none());
    }

    #[test]
    fn load_or_init_writes_default_file() {
        let dir = std::env::temp_dir().join(format!("orkai-config-{}-{}", std::process::id(), crate::util::now_ms()));
        let home = Home::at(&dir);
        home.ensure().unwrap();
        let config = Config::load_or_init(&home).unwrap();
        assert_eq!(config, Config::default());
        assert!(home.config_file().exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
