use std::path::{Path, PathBuf};

use crate::paths::{complete_directory, directory_query, matching_directories, separator_for};

pub const WHERE_COMMANDS: [&str; 2] = ["/where", "/cd"];

pub const COMMANDS: &[&str] = &[
    "/where <path>",
    "/cd <path>",
    "/who <claude|codex|terminal>",
    "/session <name>",
    "/new-session [name]",
    "/rename-session <name>",
    "/delete-session",
    "/rename <name>",
    "/kill",
    "/restart",
    "/recover",
    "/open <n>",
    "/help",
    "/quit",
];

pub fn where_argument(text: &str) -> Option<(&str, &str)> {
    let (command, rest) = text.split_once(' ')?;
    WHERE_COMMANDS
        .iter()
        .any(|c| c.eq_ignore_ascii_case(command))
        .then(|| text.split_at(command.len() + 1))
        .map(|(head, _)| (head, rest))
}

struct Cycle {
    base: String,
    candidates: Vec<String>,
    index: usize,
    current: String,
}

#[derive(Default)]
pub struct PathCompleter {
    cycle: Option<Cycle>,
    listing: Option<PathBuf>,
    suggestions: Vec<String>,
    selected: Option<usize>,
}

impl PathCompleter {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_active(&self) -> bool {
        self.listing.is_some()
    }

    pub fn listing(&self) -> Option<&Path> {
        self.listing.as_deref()
    }

    pub fn suggestions(&self) -> &[String] {
        &self.suggestions
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn refresh(&mut self, text: &str, base: &Path) {
        if self.cycle.as_ref().is_some_and(|c| c.current == text) {
            return;
        }
        self.cycle = None;
        self.selected = None;
        let query = directory_query(text, base);
        self.suggestions = matching_directories(&query.dir, &query.prefix);
        self.listing = Some(query.dir);
    }

    pub fn tab(&mut self, text: &str, base: &Path, backwards: bool) -> Option<String> {
        if let Some(cycle) = self.cycle.as_mut().filter(|c| c.current == text) {
            let len = cycle.candidates.len();
            cycle.index = if backwards { (cycle.index + len - 1) % len } else { (cycle.index + 1) % len };
            cycle.current = cycled_text(&cycle.base, &cycle.candidates[cycle.index]);
            self.selected = Some(cycle.index);
            return Some(cycle.current.clone());
        }
        let completion = complete_directory(text, base);
        if completion.text != text {
            self.refresh(&completion.text, base);
            return Some(completion.text);
        }
        if completion.candidates.len() < 2 {
            self.refresh(text, base);
            return None;
        }
        let query = directory_query(text, base);
        let index = if backwards { completion.candidates.len() - 1 } else { 0 };
        let current = cycled_text(&query.typed_base, &completion.candidates[index]);
        self.listing = Some(query.dir);
        self.suggestions = completion.candidates.clone();
        self.selected = Some(index);
        self.cycle =
            Some(Cycle { base: query.typed_base, candidates: completion.candidates, index, current: current.clone() });
        Some(current)
    }
}

fn cycled_text(base: &str, name: &str) -> String {
    format!("{base}{name}{}", separator_for(base))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tree(tag: &str) -> Tree {
        let dir =
            std::env::temp_dir().join(format!("orkai-pathcomp-{tag}-{}-{}", std::process::id(), crate::util::now_ms()));
        for sub in ["apps/web", "apps/api", "apps/admin", "docs", "libs/core", "proj-alpha", "proj-beta"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        Tree(dir)
    }

    #[test]
    fn where_argument_detects_commands_case_insensitively() {
        assert_eq!(where_argument("/where ~/x"), Some(("/where ", "~/x")));
        assert_eq!(where_argument("/CD apps"), Some(("/CD ", "apps")));
        assert_eq!(where_argument("/where "), Some(("/where ", "")));
        assert_eq!(where_argument("/where"), None);
        assert_eq!(where_argument("/whereabouts x"), None);
        assert_eq!(where_argument("fix the build"), None);
    }

    #[test]
    fn refresh_lists_matching_folders_of_the_base() {
        let t = tree("refresh");
        let mut completer = PathCompleter::default();
        completer.refresh("", &t.0);
        assert_eq!(completer.suggestions(), ["apps", "docs", "libs", "proj-alpha", "proj-beta"]);
        assert_eq!(completer.listing(), Some(t.0.as_path()));
        completer.refresh("a", &t.0);
        assert_eq!(completer.suggestions(), ["apps"]);
        completer.refresh("zzz", &t.0);
        assert!(completer.suggestions().is_empty());
        assert!(completer.is_active());
    }

    #[test]
    fn tab_enters_unique_folder_and_lists_its_children() {
        let t = tree("enter");
        let mut completer = PathCompleter::default();
        assert_eq!(completer.tab("ap", &t.0, false).as_deref(), Some("apps/"));
        assert_eq!(completer.suggestions(), ["admin", "api", "web"]);
        assert_eq!(completer.selected(), None);
    }

    #[test]
    fn tab_extends_common_prefix_then_cycles_candidates() {
        let t = tree("cycle");
        let mut completer = PathCompleter::default();
        assert_eq!(completer.tab("pr", &t.0, false).as_deref(), Some("proj-"));
        assert_eq!(completer.suggestions(), ["proj-alpha", "proj-beta"]);
        assert_eq!(completer.selected(), None);
        assert_eq!(completer.tab("proj-", &t.0, false).as_deref(), Some("proj-alpha/"));
        assert_eq!(completer.tab("proj-alpha/", &t.0, false).as_deref(), Some("proj-beta/"));
        let mut completer = PathCompleter::default();
        assert_eq!(completer.tab("apps/", &t.0, false).as_deref(), Some("apps/admin/"));
        assert_eq!(completer.selected(), Some(0));
        assert_eq!(completer.tab("apps/admin/", &t.0, false).as_deref(), Some("apps/api/"));
        assert_eq!(completer.tab("apps/api/", &t.0, false).as_deref(), Some("apps/web/"));
        assert_eq!(completer.tab("apps/web/", &t.0, false).as_deref(), Some("apps/admin/"));
        assert_eq!(completer.tab("apps/admin/", &t.0, true).as_deref(), Some("apps/web/"));
        assert_eq!(completer.suggestions(), ["admin", "api", "web"]);
    }

    #[test]
    fn typing_after_a_cycle_restarts_from_the_new_text() {
        let t = tree("reset");
        let mut completer = PathCompleter::default();
        completer.tab("apps/", &t.0, false);
        completer.refresh("apps/w", &t.0);
        assert_eq!(completer.selected(), None);
        assert_eq!(completer.suggestions(), ["web"]);
        assert_eq!(completer.tab("apps/w", &t.0, false).as_deref(), Some("apps/web/"));
    }

    #[test]
    fn tab_without_matches_changes_nothing() {
        let t = tree("none");
        let mut completer = PathCompleter::default();
        assert_eq!(completer.tab("nothing", &t.0, false), None);
        assert!(completer.suggestions().is_empty());
    }
}
