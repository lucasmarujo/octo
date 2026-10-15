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
    if trimmed == "~"
        && let Some(home) = user_home()
    {
        return home;
    }
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\"))
        && let Some(home) = user_home()
    {
        return home.join(rest);
    }
    PathBuf::from(trimmed)
}

pub const MAX_DIRECTORY_SUGGESTIONS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub text: String,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryQuery {
    pub typed_base: String,
    pub dir: PathBuf,
    pub prefix: String,
}

fn starts_with_prefix(name: &str, prefix: &str) -> bool {
    if cfg!(windows) { name.to_lowercase().starts_with(&prefix.to_lowercase()) } else { name.starts_with(prefix) }
}

pub fn common_prefix(names: &[String]) -> String {
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

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

pub fn separator_for(typed: &str) -> char {
    if typed.contains('\\') && !typed.contains('/') { '\\' } else { '/' }
}

pub fn directory_query(input: &str, base: &Path) -> DirectoryQuery {
    let typed = if input.trim() == "~" { "~/".to_string() } else { input.to_string() };
    let split = typed.rfind(is_separator).map_or(0, |i| i + 1);
    let (typed_base, prefix) = typed.split_at(split);
    let dir = if typed_base.is_empty() {
        base.to_path_buf()
    } else {
        let expanded = expand_tilde(typed_base);
        if expanded.is_relative() { base.join(expanded) } else { expanded }
    };
    DirectoryQuery { typed_base: typed_base.to_string(), dir, prefix: prefix.to_string() }
}

pub fn matching_directories(dir: &Path, prefix: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            if !starts_with_prefix(&name, prefix) || (name.starts_with('.') && !prefix.starts_with('.')) {
                return None;
            }
            let is_dir = match entry.file_type() {
                Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
                Ok(kind) => kind.is_dir(),
                Err(_) => false,
            };
            is_dir.then_some(name)
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.truncate(MAX_DIRECTORY_SUGGESTIONS);
    names
}

pub fn complete_directory(input: &str, base: &Path) -> Completion {
    let query = directory_query(input, base);
    let names = matching_directories(&query.dir, &query.prefix);
    let separator = separator_for(&query.typed_base);
    match names.len() {
        0 => Completion { text: input.to_string(), candidates: Vec::new() },
        1 => Completion { text: format!("{}{}{separator}", query.typed_base, names[0]), candidates: Vec::new() },
        _ => {
            let common = common_prefix(&names);
            let extended = if common.chars().count() > query.prefix.chars().count() { common } else { query.prefix };
            Completion { text: format!("{}{extended}", query.typed_base), candidates: names }
        }
    }
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
        let separator = separator_for(&dir.display().to_string());
        let root = format!("{}{separator}", dir.display());
        let elsewhere = std::env::temp_dir();

        let unique = complete_directory(&format!("{root}al"), &elsewhere);
        assert_eq!(unique.text, format!("{root}alpha{separator}"));
        assert!(unique.candidates.is_empty());

        let shared = complete_directory(&format!("{root}b"), &elsewhere);
        assert_eq!(shared.text, format!("{root}beta-"));
        assert_eq!(shared.candidates, vec!["beta-one".to_string(), "beta-two".to_string()]);

        let listing = complete_directory(&root, &elsewhere);
        assert_eq!(listing.candidates, vec!["alpha".to_string(), "beta-one".to_string(), "beta-two".to_string()]);

        let hidden = complete_directory(&format!("{root}.h"), &elsewhere);
        assert_eq!(hidden.text, format!("{root}.hidden{separator}"));

        let none = complete_directory(&format!("{root}zzz"), &elsewhere);
        assert_eq!(none.text, format!("{root}zzz"));
        assert!(none.candidates.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn relative_input_is_resolved_against_the_base_directory() {
        let dir = std::env::temp_dir().join(format!("orkai-relative-{}-{}", std::process::id(), crate::util::now_ms()));
        std::fs::create_dir_all(dir.join("src").join("tui")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        assert_eq!(complete_directory("sr", &dir).text, "src/");
        assert_eq!(complete_directory("src/t", &dir).text, "src/tui/");
        assert_eq!(complete_directory("", &dir).candidates, vec!["docs".to_string(), "src".to_string()]);
        let query = directory_query("src/t", &dir);
        assert_eq!(query.typed_base, "src/");
        assert_eq!(query.prefix, "t");
        assert_eq!(query.dir, dir.join("src/"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tilde_lists_the_home_directory() {
        let query = directory_query("~", Path::new("/unused"));
        assert_eq!(query.typed_base, "~/");
        assert_eq!(query.prefix, "");
        assert_eq!(query.dir, user_home().unwrap());
    }

    #[test]
    fn separator_follows_the_typed_style() {
        assert_eq!(separator_for(r"C:\Users\me\"), '\\');
        assert_eq!(separator_for("~/work/"), '/');
        assert_eq!(separator_for(""), '/');
    }

    #[test]
    fn normalize_ignores_trailing_separators() {
        assert_eq!(normalize_for_compare(Path::new("/a/b/")), normalize_for_compare(Path::new("/a/b")));
    }
}
