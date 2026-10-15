use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::fsutil;
use crate::launch::{claude_home, codex_home};
use crate::model::AgentKind;
use crate::paths::user_home;

pub const MAX_SKILLS: usize = 500;
const MAX_FRONTMATTER_LINES: usize = 64;
const MAX_COMMAND_DEPTH: usize = 3;
const SKILL_FILE: &str = "SKILL.md";
const COMMAND_EXTENSION: &str = "md";
const SYNCED_SKILLS_DIR: &str = "synced";
const CODEX_SYSTEM_SKILLS_DIR: &str = ".system";
const CODEX_ADMIN_SKILLS_DIR: &str = "/etc/codex/skills";
const CODEX_SKILL_SIGIL: char = '$';
const PLUGIN_SEPARATOR: char = ':';

struct Frontmatter {
    name: Option<String>,
    user_invocable: bool,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClaudeSettings {
    enabled_plugins: BTreeMap<String, Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct InstalledPlugins {
    plugins: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginInstall {
    install_path: PathBuf,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PluginManifest {
    name: Option<String>,
    skills: Value,
    commands: Value,
}

pub fn discover(kind: AgentKind, cwd: &Path) -> Vec<String> {
    match kind {
        AgentKind::Claude => claude_skills(claude_home().as_deref(), cwd),
        AgentKind::Codex => codex_skills(codex_home().as_deref(), user_home().as_deref(), cwd),
        AgentKind::Terminal => Vec::new(),
    }
}

pub fn invocation(kind: AgentKind, line: &str) -> String {
    let line = line.trim();
    match kind {
        AgentKind::Codex => format!("{CODEX_SKILL_SIGIL}{}", line.trim_start_matches('/')),
        _ => line.to_string(),
    }
}

pub fn claude_skills(claude_home: Option<&Path>, cwd: &Path) -> Vec<String> {
    let mut names = BTreeSet::new();
    if let Some(home) = claude_home {
        skills_in(&home.join("skills"), "", &mut names);
        skills_in(&home.join("skills").join(SYNCED_SKILLS_DIR), "", &mut names);
        commands_in(&home.join("commands"), "", &mut names);
    }
    for dir in project_dirs(cwd) {
        skills_in(&dir.join(".claude").join("skills"), "", &mut names);
        commands_in(&dir.join(".claude").join("commands"), "", &mut names);
    }
    if let Some(home) = claude_home {
        plugin_skills(home, cwd, &mut names);
    }
    names.into_iter().collect()
}

pub fn codex_skills(codex_home: Option<&Path>, user_home: Option<&Path>, cwd: &Path) -> Vec<String> {
    let mut names = BTreeSet::new();
    for dir in project_dirs(cwd) {
        skills_in(&dir.join(".agents").join("skills"), "", &mut names);
    }
    if let Some(home) = user_home {
        skills_in(&home.join(".agents").join("skills"), "", &mut names);
    }
    if let Some(home) = codex_home {
        skills_in(&home.join("skills"), "", &mut names);
        skills_in(&home.join("skills").join(CODEX_SYSTEM_SKILLS_DIR), "", &mut names);
    }
    if cfg!(unix) {
        skills_in(Path::new(CODEX_ADMIN_SKILLS_DIR), "", &mut names);
    }
    names.into_iter().collect()
}

fn project_dirs(cwd: &Path) -> Vec<&Path> {
    let Some(root) = cwd.ancestors().find(|dir| dir.join(".git").exists()) else { return vec![cwd] };
    let mut dirs: Vec<&Path> = cwd.ancestors().take_while(|dir| *dir != root).collect();
    dirs.push(root);
    dirs
}

fn add(names: &mut BTreeSet<String>, prefix: &str, name: &str) {
    let valid = !name.is_empty() && !name.starts_with('/') && !name.chars().any(char::is_whitespace);
    if valid && names.len() < MAX_SKILLS {
        names.insert(format!("{prefix}{name}"));
    }
}

fn read_frontmatter(path: &Path) -> Option<Frontmatter> {
    let file = File::open(path).ok()?;
    let mut meta = Frontmatter { name: None, user_invocable: true };
    let mut lines = BufReader::new(file).lines().map_while(Result::ok).take(MAX_FRONTMATTER_LINES);
    if lines.next().is_none_or(|first| first.trim_start_matches('\u{feff}').trim() != "---") {
        return Some(meta);
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        if key.starts_with(char::is_whitespace) {
            continue;
        }
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        match key.trim() {
            "name" if !value.is_empty() => meta.name = Some(value.to_string()),
            "user-invocable" => meta.user_invocable = !value.eq_ignore_ascii_case("false"),
            _ => {}
        }
    }
    Some(meta)
}

fn skill_at(dir: &Path, prefix: &str, names: &mut BTreeSet<String>) -> bool {
    let Some(meta) = read_frontmatter(&dir.join(SKILL_FILE)) else { return false };
    let fallback = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if meta.user_invocable {
        add(names, prefix, &meta.name.unwrap_or(fallback));
    }
    true
}

fn skills_in(dir: &Path, prefix: &str, names: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        skill_at(&entry.path(), prefix, names);
    }
}

fn command_at(file: &Path, namespace: &str, prefix: &str, names: &mut BTreeSet<String>) {
    if file.extension().and_then(|e| e.to_str()) != Some(COMMAND_EXTENSION) {
        return;
    }
    let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else { return };
    if read_frontmatter(file).is_some_and(|meta| meta.user_invocable) {
        add(names, prefix, &format!("{namespace}{stem}"));
    }
}

fn commands_in(dir: &Path, prefix: &str, names: &mut BTreeSet<String>) {
    let mut pending = vec![(dir.to_path_buf(), String::new(), 0)];
    while let Some((dir, namespace, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if depth < MAX_COMMAND_DEPTH {
                    pending.push((path, format!("{namespace}{name}{PLUGIN_SEPARATOR}"), depth + 1));
                }
            } else {
                command_at(&path, &namespace, prefix, names);
            }
        }
    }
}

fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fsutil::read_json_opt(path).ok().flatten().unwrap_or_default()
}

fn enabled_plugins(claude_home: &Path, cwd: &Path) -> BTreeSet<String> {
    let mut enabled = BTreeMap::new();
    let project = cwd.join(".claude");
    for file in [claude_home.join("settings.json"), project.join("settings.json"), project.join("settings.local.json")]
    {
        let settings: ClaudeSettings = read_json(&file);
        for (key, value) in settings.enabled_plugins {
            enabled.insert(key, value.as_bool().unwrap_or(false));
        }
    }
    enabled.into_iter().filter(|(_, on)| *on).map(|(key, _)| key).collect()
}

fn install_path(installs: Value) -> Option<PathBuf> {
    let list = match installs {
        Value::Array(list) => list,
        single => vec![single],
    };
    list.into_iter()
        .filter_map(|install| serde_json::from_value::<PluginInstall>(install).ok())
        .map(|install| install.install_path)
        .find(|path| path.is_dir())
}

fn manifest_paths(root: &Path, default_dir: &str, declared: &Value) -> Vec<PathBuf> {
    let mut paths = vec![root.join(default_dir)];
    match declared {
        Value::String(path) => paths.push(root.join(path)),
        Value::Array(list) => paths.extend(list.iter().filter_map(Value::as_str).map(|path| root.join(path))),
        _ => {}
    }
    paths
}

fn plugin_skills(claude_home: &Path, cwd: &Path, names: &mut BTreeSet<String>) {
    let enabled = enabled_plugins(claude_home, cwd);
    if enabled.is_empty() {
        return;
    }
    let installed: InstalledPlugins = read_json(&claude_home.join("plugins").join("installed_plugins.json"));
    for (key, installs) in installed.plugins {
        if !enabled.contains(&key) {
            continue;
        }
        let Some(root) = install_path(installs) else { continue };
        let manifest: PluginManifest = read_json(&root.join(".claude-plugin").join("plugin.json"));
        let plugin = manifest.name.clone().unwrap_or_else(|| key.split('@').next().unwrap_or(&key).to_string());
        let prefix = format!("{plugin}{PLUGIN_SEPARATOR}");
        for dir in manifest_paths(&root, "skills", &manifest.skills) {
            if !skill_at(&dir, &prefix, names) {
                skills_in(&dir, &prefix, names);
            }
        }
        for path in manifest_paths(&root, "commands", &manifest.commands) {
            if path.is_dir() {
                commands_in(&path, &prefix, names);
            } else {
                command_at(&path, "", &prefix, names);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tree(tag: &str) -> Tree {
        Tree(std::env::temp_dir().join(format!("orkai-skills-{tag}-{}-{}", std::process::id(), crate::util::now_ms())))
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn skill(dir: &Path, folder: &str, frontmatter: &str) {
        write(&dir.join(folder).join(SKILL_FILE), &format!("---\n{frontmatter}\n---\n\n# body\n"));
    }

    #[test]
    fn claude_lists_personal_project_and_command_skills() {
        let t = tree("claude");
        let home = t.0.join("claude-home");
        let repo = t.0.join("repo");
        let cwd = repo.join("apps").join("web");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        skill(&home.join("skills"), "review", "name: review\ndescription: Review: the diff");
        skill(&home.join("skills"), "folder-name", "description: no explicit name");
        skill(&home.join("skills"), "hidden", "name: hidden\nuser-invocable: false");
        skill(&home.join("skills").join(SYNCED_SKILLS_DIR), "synced-one", "name: \"synced-one\"");
        write(&home.join("commands").join("deploy.md"), "Deploy $ARGUMENTS");
        write(&home.join("commands").join("frontend").join("component.md"), "---\ndescription: x\n---\n");
        write(&home.join("commands").join("notes.txt"), "ignored");
        skill(&repo.join(".claude").join("skills"), "repo-root", "name: repo-root");
        skill(&cwd.join(".claude").join("skills"), "nested", "name: nested");
        skill(&t.0.join(".claude").join("skills"), "outside-repo", "name: outside-repo");
        skill(&home.join("skills"), "bad", "name: has spaces");

        let names = claude_skills(Some(&home), &cwd);
        assert_eq!(
            names,
            ["deploy", "folder-name", "frontend:component", "nested", "repo-root", "review", "synced-one"]
        );
    }

    #[test]
    fn claude_lists_enabled_plugin_skills_with_namespace() {
        let t = tree("plugins");
        let home = t.0.join("claude-home");
        let cwd = t.0.join("project");
        fs::create_dir_all(&cwd).unwrap();
        let stripe = t.0.join("cache").join("stripe");
        let design = t.0.join("cache").join("design");
        let off = t.0.join("cache").join("off");
        write(&stripe.join(".claude-plugin").join("plugin.json"), r#"{"name":"stripe"}"#);
        skill(&stripe.join("skills"), "stripe-docs", "name: stripe-docs");
        write(&stripe.join("commands").join("test-cards.md"), "cards");
        write(&design.join(".claude-plugin").join("plugin.json"), r#"{"name":"ui","skills":"./.claude/skills/"}"#);
        skill(&design.join(".claude").join("skills"), "brand", "name: brand");
        skill(&off.join("skills"), "never", "name: never");
        let installed = serde_json::json!({
            "version": 2,
            "plugins": {
                "stripe@official": [{ "scope": "user", "installPath": stripe }],
                "ui@market": { "installPath": design },
                "off@market": [{ "installPath": off }],
            }
        });
        write(&home.join("plugins").join("installed_plugins.json"), &installed.to_string());
        write(
            &home.join("settings.json"),
            r#"{"enabledPlugins":{"stripe@official":true,"ui@market":true,"off@market":true}}"#,
        );
        write(&cwd.join(".claude").join("settings.local.json"), r#"{"enabledPlugins":{"off@market":false}}"#);

        let names = claude_skills(Some(&home), &cwd);
        assert_eq!(names, ["stripe:stripe-docs", "stripe:test-cards", "ui:brand"]);
    }

    #[test]
    fn codex_lists_agents_codex_home_and_system_skills() {
        let t = tree("codex");
        let user = t.0.join("user");
        let codex = t.0.join("codex-home");
        let cwd = t.0.join("project");
        fs::create_dir_all(&cwd).unwrap();
        skill(&cwd.join(".agents").join("skills"), "repo-skill", "name: repo-skill");
        skill(&user.join(".agents").join("skills"), "maestri", "name: maestri");
        skill(&codex.join("skills"), "legacy", "name: legacy");
        skill(&codex.join("skills").join(CODEX_SYSTEM_SKILLS_DIR), "skill-creator", "name: skill-creator");

        let names = codex_skills(Some(&codex), Some(&user), &cwd);
        for expected in ["legacy", "maestri", "repo-skill", "skill-creator"] {
            assert!(names.contains(&expected.to_string()), "{expected} missing from {names:?}");
        }
    }

    #[test]
    fn missing_folders_yield_no_skills() {
        let t = tree("empty");
        assert!(claude_skills(Some(&t.0.join("nope")), &t.0.join("nowhere")).is_empty());
        assert!(discover(AgentKind::Terminal, &t.0).is_empty());
    }

    #[test]
    fn skill_list_is_bounded() {
        let t = tree("bounded");
        let home = t.0.join("claude-home");
        for n in 0..MAX_SKILLS + 20 {
            write(&home.join("commands").join(format!("c{n:04}.md")), "x");
        }
        assert_eq!(claude_skills(Some(&home), &t.0.join("cwd")).len(), MAX_SKILLS);
    }

    #[test]
    fn codex_invocations_use_the_dollar_sigil() {
        assert_eq!(invocation(AgentKind::Codex, "/skill-creator make one"), "$skill-creator make one");
        assert_eq!(invocation(AgentKind::Claude, " /review 12 "), "/review 12");
    }
}
