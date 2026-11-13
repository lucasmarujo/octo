use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::fsutil;
use crate::orchestrator::one_line;
use crate::paths::{Home, create_private_dir};
use crate::update::sha256_hex;

pub const MAX_BYTES: usize = 64 * 1024;
pub const MAX_RULE_CHARS: usize = 500;
const GLOBAL_FILE: &str = "OCTO.md";
const PROJECTS_DIR: &str = "projects";
const PROJECT_KEY_CHARS: usize = 16;
const PROJECT_SUFFIX: &str = ".md";
pub const DEFAULT: &str = "# Octo's instructions

Octo reads this file before it answers an agent or you. Write here what holds for every project.

- Prefer the smallest change that finishes the task the agent was given.
- Reading files, searching, building and running tests are fine.
- Deleting files outside the project, force pushes and changes to credentials wait for me.
- When an agent asks something the task does not settle, ask me.
";

pub fn file(home: &Home, project: Option<&str>) -> PathBuf {
    let dir = home.orchestrator_dir();
    match project {
        None => dir.join(GLOBAL_FILE),
        Some(project) => {
            let key = &sha256_hex(project.as_bytes())[..PROJECT_KEY_CHARS];
            dir.join(PROJECTS_DIR).join(format!("{key}{PROJECT_SUFFIX}"))
        }
    }
}

fn read_capped(path: &Path) -> io::Result<Option<String>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let mut data = Vec::new();
    file.take(MAX_BYTES as u64).read_to_end(&mut data)?;
    Ok(Some(String::from_utf8_lossy(&data).into_owned()))
}

fn write(path: &Path, text: &str) -> io::Result<()> {
    if text.len() > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("instructions take up to {} KiB", MAX_BYTES / 1024),
        ));
    }
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    fsutil::write_atomic(path, text.as_bytes())
}

pub fn read(home: &Home, project: Option<&str>) -> io::Result<String> {
    let path = file(home, project);
    match (read_capped(&path)?, project) {
        (Some(text), _) => Ok(text),
        (None, Some(_)) => Ok(String::new()),
        (None, None) => {
            write(&path, DEFAULT)?;
            Ok(DEFAULT.to_string())
        }
    }
}

pub fn save(home: &Home, project: Option<&str>, text: &str) -> io::Result<PathBuf> {
    let path = file(home, project);
    write(&path, text)?;
    Ok(path)
}

pub fn remember(home: &Home, project: &str, rule: &str) -> io::Result<()> {
    let rule = one_line(rule, MAX_RULE_CHARS);
    if rule.is_empty() {
        return Ok(());
    }
    let mut text = read(home, Some(project))?;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("- {rule}\n"));
    save(home, Some(project), &text).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;

    fn temp_home(tag: &str) -> (PathBuf, Home) {
        let dir = std::env::temp_dir().join(format!("orkai-octo-rules-{tag}-{}-{}", std::process::id(), now_ms()));
        (dir.clone(), Home::at(dir))
    }

    #[test]
    fn the_global_instructions_are_created_with_a_default_the_first_time_they_are_read() {
        let (dir, home) = temp_home("global");
        let path = file(&home, None);
        assert_eq!(path, dir.join("orchestrator").join("OCTO.md"));
        assert!(!path.exists());
        assert_eq!(read(&home, None).unwrap(), DEFAULT);
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT);
        fs::write(&path, "my rules").unwrap();
        assert_eq!(read(&home, None).unwrap(), "my rules", "what the user wrote is never replaced");
        fs::write(&path, "").unwrap();
        assert_eq!(read(&home, None).unwrap(), "", "a file emptied on purpose stays empty");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_project_has_its_own_file_named_after_a_digest_of_its_path() {
        let (dir, home) = temp_home("project");
        let path = file(&home, Some("dev/octo"));
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(path.parent().unwrap(), dir.join("orchestrator").join("projects"));
        assert_eq!(name.len(), 16 + ".md".len());
        assert!(name.trim_end_matches(".md").chars().all(|c| c.is_ascii_hexdigit()), "{name}");
        assert_eq!(name, format!("{}.md", &sha256_hex(b"dev/octo")[..16]));
        assert_ne!(path, file(&home, Some("dev/other")));
        assert_eq!(file(&home, Some("../../etc/passwd")).parent(), path.parent(), "a path never leaves the folder");
        assert_eq!(read(&home, Some("dev/octo")).unwrap(), "", "a project without rules has no file");
        assert!(!path.exists());
        assert_eq!(save(&home, Some("dev/octo"), "- use pnpm\n").unwrap(), path);
        assert_eq!(read(&home, Some("dev/octo")).unwrap(), "- use pnpm\n");
        assert_eq!(read(&home, Some("dev/other")).unwrap(), "");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn instructions_are_bounded_when_saved_and_when_read() {
        let (dir, home) = temp_home("bounded");
        let big = "x".repeat(MAX_BYTES + 1);
        let refused = save(&home, None, &big).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
        assert!(refused.to_string().contains("64 KiB"), "{refused}");
        assert!(save(&home, None, &big[..MAX_BYTES]).is_ok());
        fs::write(file(&home, None), &big).unwrap();
        assert_eq!(read(&home, None).unwrap().len(), MAX_BYTES, "a file grown by hand is read up to the limit");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_remembered_rule_is_appended_as_one_line_to_the_project_file() {
        let (dir, home) = temp_home("remember");
        remember(&home, "dev/octo", "  always run\nthe tests  ").unwrap();
        remember(&home, "dev/octo", "   ").unwrap();
        assert_eq!(read(&home, Some("dev/octo")).unwrap(), "- always run the tests\n", "a blank rule adds nothing");
        save(&home, Some("dev/octo"), "- always run the tests\n- written by hand").unwrap();
        remember(&home, "dev/octo", "keep the old endpoint").unwrap();
        assert_eq!(
            read(&home, Some("dev/octo")).unwrap(),
            "- always run the tests\n- written by hand\n- keep the old endpoint\n"
        );
        remember(&home, "dev/octo", &"y".repeat(MAX_RULE_CHARS * 2)).unwrap();
        let last = read(&home, Some("dev/octo")).unwrap().lines().last().unwrap().to_string();
        assert_eq!(last.chars().count(), MAX_RULE_CHARS + 2);
        save(&home, Some("dev/full"), &"z".repeat(MAX_BYTES)).unwrap();
        assert!(remember(&home, "dev/full", "one more").is_err(), "a full file takes no more rules");
        assert!(!file(&home, None).exists(), "remembering a project rule never touches the global file");
        fs::remove_dir_all(dir).unwrap();
    }
}
