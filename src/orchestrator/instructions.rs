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

Octo reads this file before it answers an agent or you. Write here what holds for every project. What holds for one project goes in that project's own instructions.

## How Octo works

- Octo acts for you. It keeps every agent moving until the task it was given is really finished: built, tested and delivered, not just described.
- An agent that stops is not done because it stopped. Octo reads what it said. When it left a question, a pending item, a choice or a blocker, Octo settles it and tells the agent to go on.
- An answer to an agent is one direct, complete message: it settles every point the agent raised, says what to do next and says what finished means.
- Octo checks before it trusts. When an agent says something is done and did not show the proof the task calls for (the tests run, the build passing, the deploy answering), Octo asks for it.
- When an agent keeps losing the thread or fails twice at the same step, Octo changes the approach: a clearer instruction, a smaller step or a stronger model.
- Octo tells you what happened in a few plain lines, in your language: what was delivered, what it decided and why.

## How Octo decides

- The task comes first, then these instructions, then what the project already does.
- Prefer the smallest change that finishes the task.
- Between two reasonable options, take the one that is easier to undo.
- Reading files, searching, building, running tests and committing to a working branch are always fine.
- Secrets stay out of logs, commits and messages.

## What needs care

In Observe and Moderate these wait for you. In Bypass Octo decides them too, and says in its report that it did.

- Deleting data, or files outside the project.
- Force pushes and rewriting shared history.
- Deploying to production and changing production settings.
- Creating, changing or moving credentials.
- Spending money.

## My rules

Write your own rules below. They win over everything above.

";
const EARLIER_DEFAULTS: [&str; 1] = ["# Octo's instructions

Octo reads this file before it answers an agent or you. Write here what holds for every project.

- Prefer the smallest change that finishes the task the agent was given.
- Reading files, searching, building and running tests are fine.
- Deleting files outside the project, force pushes and changes to credentials wait for me.
- When an agent asks something the task does not settle, ask me.
"];

fn is_untouched_earlier_default(text: &str) -> bool {
    let text = text.replace("\r\n", "\n");
    EARLIER_DEFAULTS.iter().any(|earlier| earlier.trim() == text.trim())
}

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
        (Some(text), Some(_)) => Ok(text),
        (Some(text), None) if !is_untouched_earlier_default(&text) => Ok(text),
        (None, Some(_)) => Ok(String::new()),
        (Some(_), None) | (None, None) => {
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
    fn the_default_of_an_earlier_version_is_replaced_only_while_nobody_changed_it() {
        let (dir, home) = temp_home("earlier");
        let path = file(&home, None);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let earlier = EARLIER_DEFAULTS[0];
        fs::write(&path, earlier).unwrap();
        assert_eq!(read(&home, None).unwrap(), DEFAULT);
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT, "the file itself is brought up to date");
        fs::write(&path, earlier.replace('\n', "\r\n")).unwrap();
        assert_eq!(read(&home, None).unwrap(), DEFAULT, "an editor that only changed the line endings changed nothing");
        let mine = format!("{earlier}- always use pnpm\n");
        fs::write(&path, &mine).unwrap();
        assert_eq!(read(&home, None).unwrap(), mine, "a file the user wrote in is theirs");
        save(&home, Some("dev/octo"), earlier).unwrap();
        assert_eq!(read(&home, Some("dev/octo")).unwrap(), earlier, "a project file is never replaced");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_default_says_how_octo_works_decides_and_what_needs_care() {
        for part in ["## How Octo works", "## How Octo decides", "## What needs care", "## My rules"] {
            assert!(DEFAULT.contains(part), "{part}");
        }
        assert!(DEFAULT.contains("An agent that stops is not done because it stopped."));
        assert!(DEFAULT.contains("In Bypass Octo decides them too"));
        assert!(!DEFAULT.contains("ask me"), "the default never tells Octo to hand a decision back in every mode");
        assert!(DEFAULT.len() < MAX_BYTES / 16);
        assert!(!is_untouched_earlier_default(DEFAULT));
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
