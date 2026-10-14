use std::io;
use std::path::{Path, PathBuf};

use crate::model::AgentId;

pub const HOME_ENV: &str = "ORKAI_HOME";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    root: PathBuf,
}

impl Home {
    pub fn resolve() -> io::Result<Self> {
        if let Some(dir) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::at(PathBuf::from(dir)));
        }
        let base = dirs::data_local_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "could not determine the local data directory"))?;
        Ok(Self::at(base.join("orkai")))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn ensure(&self) -> io::Result<()> {
        create_private_dir(&self.root)?;
        create_private_dir(&self.agents_dir())?;
        create_private_dir(&self.bin_dir())?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.root.join("agents")
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn agent_dir(&self, id: AgentId) -> PathBuf {
        self.agents_dir().join(id.to_string())
    }

    pub fn agent_file(&self, id: AgentId) -> PathBuf {
        self.agent_dir(id).join("agent.json")
    }

    pub fn launch_file(&self, id: AgentId) -> PathBuf {
        self.agent_dir(id).join("launch.json")
    }

    pub fn runtime_file(&self, id: AgentId) -> PathBuf {
        self.agent_dir(id).join("runtime.json")
    }

    pub fn exit_file(&self, id: AgentId) -> PathBuf {
        self.agent_dir(id).join("exit.json")
    }

    pub fn output_log(&self, id: AgentId) -> PathBuf {
        self.agent_dir(id).join("output.log")
    }

    pub fn holder_log(&self, id: AgentId) -> PathBuf {
        self.agent_dir(id).join("holder.log")
    }

    pub fn sessions_file(&self) -> PathBuf {
        self.root.join("sessions.json")
    }

    pub fn ui_file(&self) -> PathBuf {
        self.root.join("ui.json")
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.json")
    }
}

pub fn create_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    restrict_dir(path)
}

#[cfg(unix)]
pub fn restrict_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub fn restrict_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub fn user_home() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn expand_tilde(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if trimmed == "~" {
        if let Some(home) = user_home() {
            return home;
        }
    }
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        if let Some(home) = user_home() {
            return home.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub text: String,
    pub candidates: Vec<String>,
}

fn starts_with_prefix(name: &str, prefix: &str) -> bool {
    if cfg!(windows) { name.to_lowercase().starts_with(&prefix.to_lowercase()) } else { name.starts_with(prefix) }
}

fn common_prefix(names: &[String]) -> String {
    let Some(first) = names.first() else { return String::new() };
    let mut end = first.len();
    for name in &names[1..] {
        end = first
            .char_indices()
            .zip(name.chars())
            .take_while(|((_, a), b)| a == b)
            .last()
            .map(|((i, a), _)| i + a.len_utf8())
            .unwrap_or(0)
            .min(end);
    }
    first[..end].to_string()
}

pub fn complete_directory(input: &str) -> Completion {
    let unchanged = || Completion { text: input.to_string(), candidates: Vec::new() };
    let typed = if input.trim() == "~" { format!("~{}", std::path::MAIN_SEPARATOR) } else { input.to_string() };
    let ends_with_separator = typed.ends_with('/') || typed.ends_with('\\');
    let expanded = expand_tilde(&typed);
    let (dir, prefix) = if ends_with_separator {
        (expanded, String::new())
    } else {
        match (expanded.parent(), expanded.file_name()) {
            (Some(parent), Some(name)) => (parent.to_path_buf(), name.to_string_lossy().into_owned()),
            _ => return unchanged(),
        }
    };
    let Ok(entries) = std::fs::read_dir(if dir.as_os_str().is_empty() { Path::new(".") } else { &dir }) else {
        return unchanged();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|name| starts_with_prefix(name, &prefix))
        .filter(|name| prefix.starts_with('.') || !name.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    if names.is_empty() || !typed.ends_with(&prefix) {
        return unchanged();
    }
    let base = &typed[..typed.len() - prefix.len()];
    if names.len() == 1 {
        return Completion { text: format!("{base}{}{}", names[0], std::path::MAIN_SEPARATOR), candidates: Vec::new() };
    }
    let common = common_prefix(&names);
    let extended = if common.chars().count() > prefix.chars().count() { common } else { prefix.clone() };
    Completion { text: format!("{base}{extended}"), candidates: names }
}

pub fn normalize_for_compare(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let trimmed = text.trim_end_matches('/');
    let value = if trimmed.is_empty() { "/" } else { trimmed };
    if cfg!(windows) { value.to_lowercase() } else { value.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_rooted_at_home() {
        let home = Home::at("/tmp/orkai-test");
        assert_eq!(home.agent_file(7), PathBuf::from("/tmp/orkai-test/agents/7/agent.json"));
        assert_eq!(home.runtime_file(7), PathBuf::from("/tmp/orkai-test/agents/7/runtime.json"));
        assert_eq!(home.sessions_file(), PathBuf::from("/tmp/orkai-test/sessions.json"));
    }

    #[test]
    fn ensure_creates_directories() {
        let dir = std::env::temp_dir().join(format!("orkai-paths-{}", crate::util::now_ms()));
        let home = Home::at(&dir);
        home.ensure().unwrap();
        assert!(home.agents_dir().is_dir());
        assert!(home.bin_dir().is_dir());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn expand_tilde_uses_home_directory() {
        let home = user_home().unwrap();
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("~/projects"), home.join("projects"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn completes_unique_and_shared_directory_prefixes() {
        let dir = std::env::temp_dir().join(format!("orkai-complete-{}-{}", std::process::id(), crate::util::now_ms()));
        std::fs::create_dir_all(dir.join("alpha")).unwrap();
        std::fs::create_dir_all(dir.join("beta-one")).unwrap();
        std::fs::create_dir_all(dir.join("beta-two")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("afile"), b"").unwrap();
        let root = format!("{}{}", dir.display(), std::path::MAIN_SEPARATOR);

        let unique = complete_directory(&format!("{root}al"));
        assert_eq!(unique.text, format!("{root}alpha{}", std::path::MAIN_SEPARATOR));
        assert!(unique.candidates.is_empty());

        let shared = complete_directory(&format!("{root}b"));
        assert_eq!(shared.text, format!("{root}beta-"));
        assert_eq!(shared.candidates, vec!["beta-one".to_string(), "beta-two".to_string()]);

        let listing = complete_directory(&root);
        assert_eq!(listing.candidates.len(), 3);

        let none = complete_directory(&format!("{root}zzz"));
        assert_eq!(none.text, format!("{root}zzz"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn normalize_ignores_trailing_separators() {
        assert_eq!(normalize_for_compare(Path::new("/a/b/")), normalize_for_compare(Path::new("/a/b")));
    }
}
