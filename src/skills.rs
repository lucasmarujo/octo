use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::fsutil;
use crate::launch::{
    antigravity_home, claude_home, codex_home, grok_home, kilo_config_dir, kimi_home, opencode_config_dir, qwen_home,
};
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
const KIMI_SKILL_PREFIX: &str = "/skill:";
const PLUGIN_SEPARATOR: char = ':';
const PATH_SEPARATOR: char = '/';
const SKILL_DIRS: [&str; 2] = ["skill", "skills"];
const COMMAND_DIRS: [&str; 2] = ["command", "commands"];
const SHARED_SKILL_DIRS: [&str; 2] = [".claude", ".agents"];
const KILO_PROJECT_DIRS: [&str; 2] = [".kilo", ".kilocode"];
const OPENCODE_PROJECT_DIRS: [&str; 1] = [".opencode"];
const GROK_DIRS: [&str; 4] = [".grok", ".agents", ".claude", ".cursor"];
const ANTIGRAVITY_PROJECT_DIRS: [&str; 2] = [".agents", ".agent"];

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

pub type SkillFiles = BTreeMap<String, PathBuf>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub owner: AgentKind,
    pub file: PathBuf,
}

pub fn discover(kind: AgentKind, cwd: &Path) -> Vec<Skill> {
    if !kind.is_ai() {
        return Vec::new();
    }
    let others = AgentKind::ALL.into_iter().filter(|owner| owner.is_ai() && *owner != kind);
    let mut skills: BTreeMap<String, Skill> = BTreeMap::new();
    'owners: for owner in std::iter::once(kind).chain(others) {
        for (name, file) in own_skills(owner, cwd) {
            if skills.len() >= MAX_SKILLS {
                break 'owners;
            }
            skills.entry(name.clone()).or_insert_with(|| Skill { name, owner, file });
        }
    }
    skills.into_values().collect()
}

fn own_skills(kind: AgentKind, cwd: &Path) -> SkillFiles {
    match kind {
        AgentKind::Claude => claude_skills(claude_home().as_deref(), cwd),
        AgentKind::Codex => codex_skills(codex_home().as_deref(), user_home().as_deref(), cwd),
        AgentKind::Kilo => {
            let home = user_home();
            let roots = [kilo_config_dir(), home.as_ref().map(|h| h.join(".kilo"))];
            opencode_skills(&roots, home.as_deref(), &KILO_PROJECT_DIRS, cwd)
        }
        AgentKind::Opencode => {
            let home = user_home();
            let roots = [opencode_config_dir(), home.as_ref().map(|h| h.join(".opencode"))];
            opencode_skills(&roots, home.as_deref(), &OPENCODE_PROJECT_DIRS, cwd)
        }
        AgentKind::Antigravity => antigravity_skills(antigravity_home().as_deref(), cwd),
        AgentKind::Grok => grok_skills(grok_home().as_deref(), user_home().as_deref(), cwd),
        AgentKind::Qwen => qwen_skills(qwen_home().as_deref(), user_home().as_deref(), cwd),
        AgentKind::Kimi => kimi_skills(kimi_home().as_deref(), user_home().as_deref(), cwd),
        AgentKind::Terminal => SkillFiles::new(),
    }
}

pub fn invocation(kind: AgentKind, skill: &Skill, arguments: &str) -> String {
    let arguments = arguments.trim();
    if skill.owner != kind {
        return bridged_invocation(skill, arguments);
    }
    let call = match kind {
        AgentKind::Codex => format!("{CODEX_SKILL_SIGIL}{}", skill.name),
        AgentKind::Kimi => format!("{KIMI_SKILL_PREFIX}{}", skill.name),
        _ => format!("/{}", skill.name),
    };
    if arguments.is_empty() { call } else { format!("{call} {arguments}") }
}

fn bridged_invocation(skill: &Skill, arguments: &str) -> String {
    let task = format!(
        "Use the {} skill from {}: read the file \"{}\" and follow its instructions; paths in it are relative to \
         its folder.",
        skill.name,
        skill.owner.label(),
        skill.file.to_string_lossy().replace('\\', "/")
    );
    if arguments.is_empty() { task } else { format!("{task} Arguments: {arguments}") }
}

fn shared_skills(user_home: Option<&Path>, cwd: &Path, names: &mut SkillFiles) {
    for dir in SHARED_SKILL_DIRS {
        if let Some(home) = user_home {
            skills_in(&home.join(dir).join("skills"), "", names);
        }
        for project in project_dirs(cwd) {
            skills_in(&project.join(dir).join("skills"), "", names);
        }
    }
}

pub fn opencode_skills(
    roots: &[Option<PathBuf>],
    user_home: Option<&Path>,
    project: &[&str],
    cwd: &Path,
) -> SkillFiles {
    let mut names = SkillFiles::new();
    let mut dirs: Vec<PathBuf> = roots.iter().flatten().cloned().collect();
    for dir in project_dirs(cwd) {
        dirs.extend(project.iter().map(|name| dir.join(name)));
    }
    for dir in &dirs {
        for skills in SKILL_DIRS {
            skills_in(&dir.join(skills), "", &mut names);
        }
        for commands in COMMAND_DIRS {
            commands_in(&dir.join(commands), "", PATH_SEPARATOR, &mut names);
        }
    }
    shared_skills(user_home, cwd, &mut names);
    names
}

pub fn antigravity_skills(agy_home: Option<&Path>, cwd: &Path) -> SkillFiles {
    let mut names = SkillFiles::new();
    if let Some(home) = agy_home {
        skills_in(&home.join("skills"), "", &mut names);
    }
    for dir in project_dirs(cwd) {
        for project in ANTIGRAVITY_PROJECT_DIRS {
            skills_in(&dir.join(project).join("skills"), "", &mut names);
        }
    }
    names
}

pub fn grok_skills(grok_home: Option<&Path>, user_home: Option<&Path>, cwd: &Path) -> SkillFiles {
    let mut names = SkillFiles::new();
    let mut dirs: Vec<PathBuf> = grok_home.into_iter().map(Path::to_path_buf).collect();
    if let Some(home) = user_home {
        dirs.extend(GROK_DIRS.iter().skip(1).map(|dir| home.join(dir)));
    }
    for dir in project_dirs(cwd) {
        dirs.extend(GROK_DIRS.iter().map(|name| dir.join(name)));
    }
    for dir in &dirs {
        skills_in(&dir.join("skills"), "", &mut names);
        commands_in(&dir.join("commands"), "", PLUGIN_SEPARATOR, &mut names);
    }
    names
}

pub fn qwen_skills(qwen_home: Option<&Path>, user_home: Option<&Path>, cwd: &Path) -> SkillFiles {
    let mut names = SkillFiles::new();
    if let Some(home) = qwen_home {
        skills_in(&home.join("skills"), "", &mut names);
        commands_in(&home.join("commands"), "", PLUGIN_SEPARATOR, &mut names);
    }
    if let Some(home) = user_home {
        skills_in(&home.join(".agents").join("skills"), "", &mut names);
    }
    for dir in project_dirs(cwd) {
        skills_in(&dir.join(".qwen").join("skills"), "", &mut names);
        skills_in(&dir.join(".agents").join("skills"), "", &mut names);
        commands_in(&dir.join(".qwen").join("commands"), "", PLUGIN_SEPARATOR, &mut names);
    }
    names
}

pub fn kimi_skills(kimi_home: Option<&Path>, user_home: Option<&Path>, cwd: &Path) -> SkillFiles {
    let mut names = SkillFiles::new();
    if let Some(home) = kimi_home {
        skills_in(&home.join("skills"), "", &mut names);
    }
    if let Some(home) = user_home {
        skills_in(&home.join(".agents").join("skills"), "", &mut names);
    }
    for dir in project_dirs(cwd) {
        skills_in(&dir.join(".kimi-code").join("skills"), "", &mut names);
        skills_in(&dir.join(".agents").join("skills"), "", &mut names);
    }
    names
}

pub fn claude_skills(claude_home: Option<&Path>, cwd: &Path) -> SkillFiles {
    let mut names = SkillFiles::new();
    if let Some(home) = claude_home {
        skills_in(&home.join("skills"), "", &mut names);
        skills_in(&home.join("skills").join(SYNCED_SKILLS_DIR), "", &mut names);
        commands_in(&home.join("commands"), "", PLUGIN_SEPARATOR, &mut names);
    }
    for dir in project_dirs(cwd) {
        skills_in(&dir.join(".claude").join("skills"), "", &mut names);
        commands_in(&dir.join(".claude").join("commands"), "", PLUGIN_SEPARATOR, &mut names);
    }
    if let Some(home) = claude_home {
        plugin_skills(home, cwd, &mut names);
    }
    names
}

pub fn codex_skills(codex_home: Option<&Path>, user_home: Option<&Path>, cwd: &Path) -> SkillFiles {
    let mut names = SkillFiles::new();
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
    names
}

fn project_dirs(cwd: &Path) -> Vec<&Path> {
    let Some(root) = cwd.ancestors().find(|dir| dir.join(".git").exists()) else { return vec![cwd] };
    let mut dirs: Vec<&Path> = cwd.ancestors().take_while(|dir| *dir != root).collect();
    dirs.push(root);
    dirs
}

fn add(names: &mut SkillFiles, prefix: &str, name: &str, file: &Path) {
    let valid = !name.is_empty() && !name.starts_with('/') && !name.chars().any(char::is_whitespace);
    if valid && names.len() < MAX_SKILLS {
        names.entry(format!("{prefix}{name}")).or_insert_with(|| file.to_path_buf());
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

fn skill_at(dir: &Path, prefix: &str, names: &mut SkillFiles) -> bool {
    let file = dir.join(SKILL_FILE);
    let Some(meta) = read_frontmatter(&file) else { return false };
    let fallback = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if meta.user_invocable {
        add(names, prefix, &meta.name.unwrap_or(fallback), &file);
    }
    true
}

fn skills_in(dir: &Path, prefix: &str, names: &mut SkillFiles) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        skill_at(&entry.path(), prefix, names);
    }
}

fn command_at(file: &Path, namespace: &str, prefix: &str, names: &mut SkillFiles) {
    if file.extension().and_then(|e| e.to_str()) != Some(COMMAND_EXTENSION) {
        return;
    }
    let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else { return };
    if read_frontmatter(file).is_some_and(|meta| meta.user_invocable) {
        add(names, prefix, &format!("{namespace}{stem}"), file);
    }
}

fn commands_in(dir: &Path, prefix: &str, separator: char, names: &mut SkillFiles) {
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
                    pending.push((path, format!("{namespace}{name}{separator}"), depth + 1));
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

fn plugin_skills(claude_home: &Path, cwd: &Path, names: &mut SkillFiles) {
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
                commands_in(&path, &prefix, PLUGIN_SEPARATOR, names);
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

    fn names(found: SkillFiles) -> Vec<String> {
        found.into_keys().collect()
    }

    fn named<'a>(skills: &'a [Skill], name: &str) -> &'a Skill {
        skills.iter().find(|skill| skill.name == name).unwrap_or_else(|| panic!("{name} missing from {skills:?}"))
    }

    fn owned(name: &str, owner: AgentKind, file: &Path) -> Skill {
        Skill { name: name.into(), owner, file: file.to_path_buf() }
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

        let names = names(claude_skills(Some(&home), &cwd));
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

        let names = names(claude_skills(Some(&home), &cwd));
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

        let names = names(codex_skills(Some(&codex), Some(&user), &cwd));
        for expected in ["legacy", "maestri", "repo-skill", "skill-creator"] {
            assert!(names.contains(&expected.to_string()), "{expected} missing from {names:?}");
        }
    }

    #[test]
    fn opencode_and_kilo_list_their_own_and_shared_skills_and_commands() {
        let t = tree("opencode");
        let user = t.0.join("user");
        let config = t.0.join("config").join("opencode");
        let repo = t.0.join("repo");
        let cwd = repo.join("web");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        skill(&config.join("skills"), "global-skill", "name: global-skill");
        skill(&config.join("skill"), "legacy-skill", "name: legacy-skill");
        write(&config.join("commands").join("git").join("commit.md"), "Commit $ARGUMENTS");
        write(&repo.join(".opencode").join("command").join("deploy.md"), "Deploy");
        skill(&user.join(".claude").join("skills"), "from-claude", "name: from-claude");
        skill(&cwd.join(".agents").join("skills"), "repo-agents", "name: repo-agents");
        skill(&repo.join(".kilo").join("skills"), "only-kilo", "name: only-kilo");

        let opencode = names(opencode_skills(&[Some(config.clone()), None], Some(&user), &OPENCODE_PROJECT_DIRS, &cwd));
        assert_eq!(opencode, ["deploy", "from-claude", "git/commit", "global-skill", "legacy-skill", "repo-agents"]);
        let kilo = names(opencode_skills(&[None], Some(&user), &KILO_PROJECT_DIRS, &cwd));
        assert_eq!(kilo, ["from-claude", "only-kilo", "repo-agents"]);
    }

    #[test]
    fn grok_qwen_kimi_and_antigravity_read_their_folders() {
        let t = tree("others");
        let user = t.0.join("user");
        let cwd = t.0.join("project");
        fs::create_dir_all(&cwd).unwrap();
        skill(&user.join(".agents").join("skills"), "shared", "name: shared");
        skill(&cwd.join(".agents").join("skills"), "repo", "name: repo");

        let grok = user.join(".grok");
        skill(&grok.join("skills"), "grok-own", "name: grok-own");
        skill(&user.join(".cursor").join("skills"), "from-cursor", "name: from-cursor");
        write(&cwd.join(".claude").join("commands").join("ship.md"), "Ship it");
        assert_eq!(
            names(grok_skills(Some(&grok), Some(&user), &cwd)),
            ["from-cursor", "grok-own", "repo", "shared", "ship"]
        );

        let qwen = user.join(".qwen");
        skill(&qwen.join("skills"), "qwen-own", "name: qwen-own");
        write(&qwen.join("commands").join("git").join("commit.md"), "---\ndescription: c\n---\n");
        write(&cwd.join(".qwen").join("commands").join("old.toml"), "prompt = 'x'");
        assert_eq!(names(qwen_skills(Some(&qwen), Some(&user), &cwd)), ["git:commit", "qwen-own", "repo", "shared"]);

        let kimi = user.join(".kimi-code");
        skill(&kimi.join("skills"), "kimi-own", "name: kimi-own");
        skill(&cwd.join(".kimi-code").join("skills"), "kimi-repo", "name: kimi-repo");
        assert_eq!(names(kimi_skills(Some(&kimi), Some(&user), &cwd)), ["kimi-own", "kimi-repo", "repo", "shared"]);

        let agy = user.join(".gemini").join("antigravity-cli");
        skill(&agy.join("skills"), "agy-own", "name: agy-own");
        skill(&cwd.join(".agent").join("skills"), "legacy", "name: legacy");
        assert_eq!(names(antigravity_skills(Some(&agy), &cwd)), ["agy-own", "legacy", "repo"]);
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
    fn skills_remember_the_file_that_defines_them() {
        let t = tree("files");
        let home = t.0.join("claude-home");
        skill(&home.join("skills"), "review", "name: review");
        write(&home.join("commands").join("git").join("commit.md"), "Commit");

        let found = claude_skills(Some(&home), &t.0.join("cwd"));
        assert_eq!(found["review"], home.join("skills").join("review").join(SKILL_FILE));
        assert_eq!(found["git:commit"], home.join("commands").join("git").join("commit.md"));
    }

    #[test]
    fn every_agent_gets_the_skills_of_the_others_and_keeps_its_own() {
        let t = tree("bridge");
        let repo = t.0.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        skill(&repo.join(".claude").join("skills"), "orkai-bridge-claude", "name: orkai-bridge-claude");
        skill(&repo.join(".agents").join("skills"), "orkai-bridge-agents", "name: orkai-bridge-agents");
        skill(&repo.join(".kimi-code").join("skills"), "orkai-bridge-kimi", "name: orkai-bridge-kimi");
        skill(&repo.join(".claude").join("skills"), "orkai-bridge-twin", "name: orkai-bridge-twin");
        skill(&repo.join(".qwen").join("skills"), "orkai-bridge-twin", "name: orkai-bridge-twin");
        let claude_twin = repo.join(".claude").join("skills").join("orkai-bridge-twin").join(SKILL_FILE);

        let codex = discover(AgentKind::Codex, &repo);
        assert_eq!(
            named(&codex, "orkai-bridge-claude"),
            &owned(
                "orkai-bridge-claude",
                AgentKind::Claude,
                &repo.join(".claude").join("skills").join("orkai-bridge-claude").join(SKILL_FILE)
            )
        );
        assert_eq!(named(&codex, "orkai-bridge-agents").owner, AgentKind::Codex);
        assert_eq!(named(&codex, "orkai-bridge-kimi").owner, AgentKind::Kimi);
        assert_eq!(named(&codex, "orkai-bridge-twin"), &owned("orkai-bridge-twin", AgentKind::Claude, &claude_twin));

        let claude = discover(AgentKind::Claude, &repo);
        assert_eq!(named(&claude, "orkai-bridge-claude").owner, AgentKind::Claude);
        assert_eq!(named(&claude, "orkai-bridge-agents").owner, AgentKind::Codex);
        assert_eq!(named(&claude, "orkai-bridge-kimi").owner, AgentKind::Kimi);

        let qwen = discover(AgentKind::Qwen, &repo);
        let qwen_twin = repo.join(".qwen").join("skills").join("orkai-bridge-twin").join(SKILL_FILE);
        assert_eq!(named(&qwen, "orkai-bridge-twin"), &owned("orkai-bridge-twin", AgentKind::Qwen, &qwen_twin));
        assert_eq!(named(&qwen, "orkai-bridge-agents").owner, AgentKind::Qwen);

        for kind in AgentKind::ALL.into_iter().filter(|kind| kind.is_ai()) {
            let skills = discover(kind, &repo);
            assert_eq!(skills.iter().filter(|skill| skill.name == "orkai-bridge-twin").count(), 1, "{kind:?}");
            assert!(skills.windows(2).all(|pair| pair[0].name < pair[1].name), "{kind:?} is not sorted");
        }
        assert!(discover(AgentKind::Terminal, &repo).is_empty());
    }

    #[test]
    fn bridged_skills_fill_the_list_only_up_to_its_bound() {
        let t = tree("bridge-bound");
        let repo = t.0.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        for n in 0..MAX_SKILLS + 20 {
            write(&repo.join(".claude").join("commands").join(format!("orkai-c{n:04}.md")), "x");
        }
        skill(&repo.join(".agents").join("skills"), "orkai-bridge-agents", "name: orkai-bridge-agents");

        let claude = discover(AgentKind::Claude, &repo);
        assert_eq!(claude.len(), MAX_SKILLS);
        assert!(claude.iter().all(|skill| skill.owner == AgentKind::Claude));
        assert_eq!(discover(AgentKind::Codex, &repo).len(), MAX_SKILLS);
    }

    #[test]
    fn own_skills_are_invoked_in_each_agents_format() {
        let file = Path::new("SKILL.md");
        let creator = owned("skill-creator", AgentKind::Codex, file);
        assert_eq!(invocation(AgentKind::Codex, &creator, " make one "), "$skill-creator make one");
        assert_eq!(invocation(AgentKind::Claude, &owned("review", AgentKind::Claude, file), "12"), "/review 12");
        assert_eq!(invocation(AgentKind::Claude, &owned("review", AgentKind::Claude, file), " "), "/review");
        assert_eq!(invocation(AgentKind::Kimi, &owned("review", AgentKind::Kimi, file), "12"), "/skill:review 12");
        let commit = owned("git/commit", AgentKind::Opencode, file);
        assert_eq!(invocation(AgentKind::Opencode, &commit, "now"), "/git/commit now");
    }

    #[test]
    fn skills_of_other_agents_are_handed_over_as_their_file() {
        let file = std::env::temp_dir().join("claude-home").join("skills").join("frontend-design").join(SKILL_FILE);
        let design = owned("frontend-design", AgentKind::Claude, &file);
        let expected = format!(
            "Use the frontend-design skill from claude: read the file \"{}\" and follow its instructions; paths in it \
             are relative to its folder. Arguments: build the landing page",
            file.to_string_lossy().replace('\\', "/")
        );
        assert_eq!(invocation(AgentKind::Codex, &design, " build the landing page "), expected);
        for kind in AgentKind::ALL.into_iter().filter(|kind| kind.is_ai() && *kind != AgentKind::Claude) {
            let task = invocation(kind, &design, "");
            assert!(task.starts_with("Use the frontend-design skill from claude: read the file"), "{kind:?}: {task}");
            assert!(task.ends_with("relative to its folder."), "{kind:?}: {task}");
            assert!(!task.contains(['$', '@', '\\', '\n', '\r']), "{kind:?}: {task}");
        }
        let creator = owned("skill-creator", AgentKind::Codex, &file);
        assert!(invocation(AgentKind::Kimi, &creator, "x").starts_with("Use the skill-creator skill from codex"));
    }
}
