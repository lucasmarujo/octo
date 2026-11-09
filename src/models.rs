use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::fsutil;
use crate::launch::{claude_home, codex_home};
use crate::model::AgentKind;

pub const MAX_MODELS: usize = 32;
pub const MAX_MODEL_ID_CHARS: usize = 128;
const CLAUDE_CATALOG_DIR: &str = "cache/model-catalog";
const CLAUDE_CATALOG_SUFFIX: &str = "-cc.json";
const CLAUDE_MAIN_SECTION: &str = "main";
const CODEX_MODELS_CACHE: &str = "models_cache.json";
const CODEX_LISTED: &str = "list";
const MODEL_ID_PUNCTUATION: &[char] = &['-', '.', '_', ':', '/', '@', '[', ']'];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClaudeCatalogFile {
    fetched_at: u64,
    catalog: ClaudeCatalog,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeCatalog {
    config: ClaudeCatalogConfig,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeCatalogConfig {
    models: Vec<ClaudeModel>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeModel {
    id: String,
    name: String,
    description: Option<String>,
    section: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexModelsCache {
    models: Vec<CodexModel>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodexModel {
    slug: String,
    display_name: Option<String>,
    description: Option<String>,
    visibility: String,
    priority: i64,
}

pub fn available(kind: AgentKind) -> Vec<Model> {
    match kind {
        AgentKind::Claude => claude_home().map(|home| claude_models(&home)).unwrap_or_default(),
        AgentKind::Codex => codex_home().map(|home| codex_models(&home)).unwrap_or_default(),
        AgentKind::Kilo
        | AgentKind::Opencode
        | AgentKind::Antigravity
        | AgentKind::Grok
        | AgentKind::Qwen
        | AgentKind::Kimi
        | AgentKind::Pi
        | AgentKind::Terminal => Vec::new(),
    }
}

pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= MAX_MODEL_ID_CHARS
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || MODEL_ID_PUNCTUATION.contains(&c))
}

fn claude_models(claude_home: &Path) -> Vec<Model> {
    let Ok(entries) = fs::read_dir(claude_home.join(CLAUDE_CATALOG_DIR)) else { return Vec::new() };
    let newest = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(CLAUDE_CATALOG_SUFFIX))
        })
        .filter_map(|path| fsutil::read_json_opt::<ClaudeCatalogFile>(&path).ok().flatten())
        .max_by_key(|file| file.fetched_at);
    let Some(file) = newest else { return Vec::new() };
    let models = file
        .catalog
        .config
        .models
        .into_iter()
        .filter(|model| model.section.as_deref() == Some(CLAUDE_MAIN_SECTION))
        .map(|model| Model {
            name: named(model.name, &model.id),
            description: model.description.unwrap_or_default(),
            id: model.id,
        });
    listed(models)
}

fn codex_models(codex_home: &Path) -> Vec<Model> {
    let Some(cache) = fsutil::read_json_opt::<CodexModelsCache>(&codex_home.join(CODEX_MODELS_CACHE)).ok().flatten()
    else {
        return Vec::new();
    };
    let mut models: Vec<CodexModel> =
        cache.models.into_iter().filter(|model| model.visibility == CODEX_LISTED).collect();
    models.sort_by_key(|model| model.priority);
    listed(models.into_iter().map(|model| Model {
        name: named(model.display_name.unwrap_or_default(), &model.slug),
        description: model.description.unwrap_or_default(),
        id: model.slug,
    }))
}

fn named(name: String, id: &str) -> String {
    if name.trim().is_empty() { id.to_string() } else { name.trim().to_string() }
}

fn listed(models: impl Iterator<Item = Model>) -> Vec<Model> {
    let mut found: Vec<Model> = Vec::new();
    for model in models.filter(|model| is_valid_id(&model.id)) {
        if found.len() >= MAX_MODELS {
            break;
        }
        if !found.iter().any(|known| known.id == model.id) {
            found.push(model);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    struct Tree(PathBuf);

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tree(tag: &str) -> Tree {
        let dir =
            std::env::temp_dir().join(format!("orkai-models-{tag}-{}-{}", std::process::id(), crate::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        Tree(dir)
    }

    fn write(path: &Path, value: serde_json::Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }

    fn claude_catalog(fetched_at: u64, models: serde_json::Value) -> serde_json::Value {
        json!({ "version": 2, "fetchedAt": fetched_at, "catalog": { "surface": "cc", "config": { "id": "cc", "models": models } } })
    }

    fn ids(models: &[Model]) -> Vec<&str> {
        models.iter().map(|model| model.id.as_str()).collect()
    }

    #[test]
    fn claude_lists_the_main_models_of_its_newest_code_catalog() {
        let home = tree("claude");
        let catalogs = home.0.join(CLAUDE_CATALOG_DIR);
        write(
            &catalogs.join("org-account-cc.json"),
            claude_catalog(
                2_000,
                json!([
                    { "id": "claude-opus-5-5", "name": "Opus 5.5", "description": "For complex work", "section": "main" },
                    { "id": "claude-haiku-5-5", "name": "Haiku 5.5", "section": "main" },
                    { "id": "claude-opus-4-6", "name": "Opus 4.6", "section": "overflow" },
                    { "id": "--settings", "name": "Not a model", "section": "main" }
                ]),
            ),
        );
        write(
            &catalogs.join("old-account-cc.json"),
            claude_catalog(1_000, json!([{ "id": "claude-old", "section": "main" }])),
        );
        write(
            &catalogs.join("org-account-ccd.json"),
            claude_catalog(9_000, json!([{ "id": "desktop-only", "section": "main" }])),
        );
        fs::write(catalogs.join("broken-cc.json"), "{ not json").unwrap();
        let models = claude_models(&home.0);
        assert_eq!(ids(&models), ["claude-opus-5-5", "claude-haiku-5-5"]);
        assert_eq!(models[0].name, "Opus 5.5");
        assert_eq!(models[0].description, "For complex work");
        assert_eq!(models[1].description, "");
    }

    #[test]
    fn codex_lists_the_models_its_cli_shows_in_its_own_order() {
        let home = tree("codex");
        write(
            &home.0.join(CODEX_MODELS_CACHE),
            json!({ "fetched_at": "2026-11-07", "models": [
                { "slug": "gpt-reserve", "display_name": "GPT-Reserve", "visibility": "hide", "priority": 1 },
                { "slug": "gpt-5.6-luna", "display_name": "GPT-5.6-Luna", "visibility": "list", "priority": 4 },
                { "slug": "gpt-6-luna", "display_name": "GPT-6-Luna", "description": "Newest", "visibility": "list", "priority": 2 },
                { "slug": "gpt-5.6-terra", "display_name": "", "visibility": "list", "priority": 3 }
            ] }),
        );
        let models = codex_models(&home.0);
        assert_eq!(ids(&models), ["gpt-6-luna", "gpt-5.6-terra", "gpt-5.6-luna"]);
        assert_eq!(models[0].description, "Newest");
        assert_eq!(models[1].name, "gpt-5.6-terra", "a model without a name shows its id");
    }

    #[test]
    fn a_long_catalog_is_cut_and_repeated_models_are_listed_once() {
        let home = tree("long");
        let many: Vec<serde_json::Value> = (0..MAX_MODELS + 10)
            .map(|n| json!({ "slug": format!("model-{}", n % (MAX_MODELS + 5)), "visibility": "list", "priority": n }))
            .collect();
        write(&home.0.join(CODEX_MODELS_CACHE), json!({ "models": many }));
        let models = codex_models(&home.0);
        assert_eq!(models.len(), MAX_MODELS);
        assert_eq!(ids(&models).iter().filter(|id| **id == "model-0").count(), 1);
    }

    #[test]
    fn missing_or_unreadable_catalogs_list_nothing() {
        let home = tree("missing");
        assert!(claude_models(&home.0).is_empty());
        assert!(codex_models(&home.0).is_empty());
        fs::write(home.0.join(CODEX_MODELS_CACHE), "[]").unwrap();
        assert!(codex_models(&home.0).is_empty());
        assert!(available(AgentKind::Terminal).is_empty());
        assert!(available(AgentKind::Kimi).is_empty());
    }

    #[test]
    fn only_plain_model_ids_reach_the_command_line() {
        for id in [
            "claude-opus-5-5",
            "claude-fable-5-1[1m]",
            "gpt-5.6-terra",
            "opus",
            "us.anthropic.claude:1",
            "org/model@v2",
        ] {
            assert!(is_valid_id(id), "{id}");
        }
        let too_long = "a".repeat(MAX_MODEL_ID_CHARS + 1);
        for id in ["", "-m", "--dangerously-skip-permissions", "opus sonnet", "opus;rm", "modelo-é", too_long.as_str()]
        {
            assert!(!is_valid_id(id), "{id}");
        }
    }
}
