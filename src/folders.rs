//! Folder browsing behind the desktop and phone pickers: listing, deep search and places.

use std::collections::VecDeque;
use std::fs::DirEntry;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_LISTED: usize = 2000;
pub const MAX_HITS: usize = 60;
const SEARCH_DEPTH: usize = 7;
const ENOUGH_WHOLE: usize = 3;
const SEARCH_VISITS: usize = 60_000;
const SEARCH_BUDGET: Duration = Duration::from_millis(700);

// Build output, caches and package stores: never a folder anyone picks, and they dominate a walk.
const HEAVY: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "__pycache__",
    ".venv",
    "venv",
    ".cache",
    ".npm",
    ".pnpm-store",
    ".cargo",
    ".rustup",
    ".gradle",
    ".m2",
    ".next",
    ".nuxt",
    ".turbo",
    "dist",
    "build",
    "vendor",
    "Pods",
    "DerivedData",
    ".Trash",
];
// Only skipped right under a filesystem root, so ~/dev or ~/build-tools still show up.
const SYSTEM_ROOTS: &[&str] = &[
    "proc",
    "sys",
    "dev",
    "run",
    "boot",
    "snap",
    "lost+found",
    "System",
    "private",
    "cores",
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "$Recycle.Bin",
    "System Volume Information",
    "Recovery",
];
const CODE_FOLDERS: &[&str] =
    &["Developer", "dev", "code", "Code", "projects", "Projects", "repos", "workspace", "src", "git", "GitHub", "work"];

pub struct Listing {
    pub dirs: Vec<(String, PathBuf, bool)>,
    pub truncated: bool,
}

pub struct Hit {
    pub name: String,
    pub path: PathBuf,
    pub rel: String,
    pub git: bool,
    /// The repository the folder belongs to, itself included.
    pub repo: Option<PathBuf>,
    score: i64,
    whole: bool,
    depth: usize,
}

pub struct Place {
    pub kind: &'static str,
    pub name: String,
    pub path: PathBuf,
}

pub fn is_repo(dir: &Path) -> bool {
    // `.git` is a folder in a clone and a file in a worktree.
    dir.join(".git").exists()
}

#[cfg(windows)]
fn hidden_attribute(entry: &DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    entry.metadata().is_ok_and(|meta| meta.file_attributes() & 0x2 != 0)
}

#[cfg(not(windows))]
fn hidden_attribute(_entry: &DirEntry) -> bool {
    false
}

fn subfolders(dir: &Path, hidden: bool, follow_links: bool) -> impl Iterator<Item = (String, PathBuf)> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(move |entry| {
        let name = entry.file_name().to_str()?.to_string();
        if !hidden && (name.starts_with('.') || hidden_attribute(&entry)) {
            return None;
        }
        let is_dir = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => follow_links && entry.path().is_dir(),
            Ok(kind) => kind.is_dir(),
            Err(_) => false,
        };
        is_dir.then(|| (name, entry.path()))
    })
}

pub fn list(dir: &Path, hidden: bool) -> Listing {
    let mut dirs: Vec<_> =
        subfolders(dir, hidden, true).map(|(name, path)| (name, path.clone(), is_repo(&path))).collect();
    dirs.sort_by_cached_key(|(name, ..)| name.to_lowercase());
    let truncated = dirs.len() > MAX_LISTED;
    dirs.truncate(MAX_LISTED);
    Listing { dirs, truncated }
}

fn skipped(name: &str, under_root: bool) -> bool {
    HEAVY.contains(&name)
        || (under_root && SYSTEM_ROOTS.iter().any(|system| system.eq_ignore_ascii_case(name)))
        || (cfg!(target_os = "macos") && name == "Library")
        || (cfg!(windows) && name.eq_ignore_ascii_case("AppData"))
}

// macOS apps and bundles are folders on disk but files to the person; their insides are noise.
fn is_package(name: &str) -> bool {
    cfg!(target_os = "macos")
        && [".app", ".framework", ".bundle", ".xcodeproj", ".xcworkspace", ".photoslibrary", ".plugin"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

fn boundary(chars: &[char], at: usize) -> bool {
    at == 0
        || matches!(chars[at - 1], '-' | '_' | '.' | ' ' | '/')
        || (chars[at - 1].is_lowercase() && chars[at].is_uppercase())
}

/// How well `query` names `text`: exact, then prefix, then a word start, then anywhere, then letters in order.
pub fn score(query: &str, text: &str) -> Option<i64> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let original: Vec<char> = text.chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    if q.is_empty() || q.len() > t.len() || original.len() != t.len() {
        return None;
    }
    let extra = (t.len() - q.len()) as i64;
    if q == t {
        return Some(1000);
    }
    if t.starts_with(&q) {
        return Some(800 - extra);
    }
    let found = (0..=t.len() - q.len()).filter(|&at| t[at..at + q.len()] == q[..]).collect::<Vec<_>>();
    if let Some(&at) = found.iter().find(|&&at| boundary(&original, at)) {
        return Some(650 - at as i64 - extra / 2);
    }
    if let Some(&at) = found.first() {
        return Some(500 - at as i64 - extra / 2);
    }
    // Letters in order count only from the start of a word, or "octo" would match "collections".
    let first = (0..t.len()).find(|&at| t[at] == q[0] && boundary(&original, at))?;
    let (mut at, mut gaps, mut starts) = (first + 1, 0i64, 1i64);
    for &wanted in &q[1..] {
        let skip = t[at..].iter().position(|&c| c == wanted)?;
        gaps += skip as i64;
        starts += i64::from(boundary(&original, at + skip));
        at += skip + 1;
    }
    let span = (at - first) as i64;
    (span <= q.len() as i64 * 3).then(|| 300 - gaps * 6 + starts * 25 - extra / 4)
}

/// Every folder under `root` whose name (or path from root, when the query has a slash) matches, best first.
/// Stops after a fixed time and number of folders so a huge disk answers fast; `truncated` says so.
pub fn search(root: &Path, query: &str, hidden: bool) -> (Vec<Hit>, bool) {
    let query = query.trim().replace('\\', "/");
    // With a slash the last part names the folder and the rest must appear, in order, in the path above it.
    let (head, tail) = query.rsplit_once('/').map_or(("", query.as_str()), |(head, tail)| (head, tail));
    let head = head.to_lowercase();
    let tail_lower = tail.to_lowercase();
    let started = Instant::now();
    let root_repo = root.ancestors().find(|folder| is_repo(folder)).map(Path::to_path_buf);
    let mut queue = VecDeque::from([(root.to_path_buf(), String::new(), 0usize)]);
    let (mut hits, mut visits, mut truncated) = (Vec::new(), 0usize, false);
    while let Some((dir, rel, depth)) = queue.pop_front() {
        let under_root = dir.parent().is_none();
        for (name, path) in subfolders(&dir, hidden, false) {
            visits += 1;
            if skipped(&name, under_root) {
                continue;
            }
            let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let matched = score(tail, &name).filter(|_| head.is_empty() || rel.to_lowercase().contains(&head));
            if let Some(points) = matched {
                let git = is_repo(&path);
                let score = points - depth as i64 * 12 + if git { 40 } else { 0 };
                let repo = path
                    .ancestors()
                    .take(depth + 2)
                    .find(|folder| is_repo(folder))
                    .map(Path::to_path_buf)
                    .or_else(|| root_repo.clone());
                hits.push(Hit {
                    name: name.clone(),
                    path: path.clone(),
                    rel: rel.clone(),
                    git,
                    repo,
                    score,
                    whole: name.to_lowercase().contains(&tail_lower),
                    depth,
                });
            }
            if depth + 1 < SEARCH_DEPTH && !is_package(&name) {
                queue.push_back((path, child_rel, depth + 1));
            }
        }
        if visits >= SEARCH_VISITS || started.elapsed() >= SEARCH_BUDGET {
            truncated = !queue.is_empty();
            break;
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.depth.cmp(&b.depth))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    if hits.iter().filter(|hit| hit.whole).count() >= ENOUGH_WHOLE {
        hits.retain(|hit| hit.whole);
    }
    hits.truncate(MAX_HITS);
    (hits, truncated)
}

/// The places a picker offers in its side bar, in the words of each system.
pub fn places() -> Vec<Place> {
    let mut places = Vec::new();
    let mut add = |kind: &'static str, name: String, path: PathBuf| {
        if path.is_dir() && !places.iter().any(|place: &Place| place.path == path) {
            places.push(Place { kind, name, path });
        }
    };
    let name_of = |path: &Path| {
        path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned())
    };
    let home = dirs::home_dir();
    if let Some(home) = &home {
        add("home", name_of(home), home.clone());
    }
    for (kind, dir) in
        [("desktop", dirs::desktop_dir()), ("documents", dirs::document_dir()), ("downloads", dirs::download_dir())]
    {
        if let Some(dir) = dir {
            add(kind, name_of(&dir), dir);
        }
    }
    if let Some(home) = &home {
        for name in CODE_FOLDERS {
            add("code", (*name).to_string(), home.join(name));
        }
    }
    for (name, path) in volumes() {
        add("volume", name, path);
    }
    places
}

#[cfg(windows)]
fn volumes() -> Vec<(String, PathBuf)> {
    // A and B are floppy letters that can stall the first probe.
    (b'C'..=b'Z')
        .map(|letter| (format!("{}:", letter as char), PathBuf::from(format!("{}:\\", letter as char))))
        .filter(|(_, path)| path.exists())
        .collect()
}

#[cfg(not(windows))]
fn volumes() -> Vec<(String, PathBuf)> {
    let mut found = vec![("/".to_string(), PathBuf::from("/"))];
    let user = std::env::var("USER").unwrap_or_default();
    let mounts = if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Volumes")]
    } else {
        vec![PathBuf::from("/media").join(user), PathBuf::from("/mnt")]
    };
    for mount in mounts {
        // On a Mac the boot disk is a symlink to / inside /Volumes; links are left out.
        found.extend(subfolders(&mount, false, false));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("octo-folders-{tag}-{}-{}", std::process::id(), crate::util::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_score_ranks_exact_then_prefix_then_word_start_then_anywhere_then_letters_in_order() {
        let ranked = ["octo", "octo-app", "the-octo", "proctor", "o-c-t-o"].map(|name| score("octo", name).unwrap());
        assert!(ranked.windows(2).all(|pair| pair[0] > pair[1]), "{ranked:?}");
        assert_eq!(score("OCTO", "Octo"), Some(1000), "case does not matter");
        assert!(score("dsys", "design-system").is_some(), "letters in order across words");
        assert!(score("xyz", "octo").is_none());
        assert!(score("ao", "a-very-long-name-with-o").is_none(), "letters spread too far apart do not count");
    }

    #[test]
    fn the_search_finds_deep_folders_ranks_repositories_first_and_skips_build_output() {
        let root = temp("search");
        for dir in ["work/octo/.git", "notes/octo-ideas", "work/app/node_modules/octo", ".hidden/octo"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let (hits, truncated) = search(&root, "octo", false);
        assert!(!truncated);
        let found: Vec<(&str, &str, bool)> =
            hits.iter().map(|hit| (hit.name.as_str(), hit.rel.as_str(), hit.git)).collect();
        assert_eq!(found, [("octo", "work", true), ("octo-ideas", "notes", false)]);
        assert_eq!(hits[0].repo.as_deref(), Some(&*root.join("work/octo")));
        std::fs::create_dir_all(root.join("work/octo/octopus")).unwrap();
        let (hits, _) = search(&root, "octopus", false);
        assert_eq!(hits[0].repo.as_deref(), Some(&*root.join("work/octo")), "a folder inside a repository names it");
        let (hits, _) = search(&root, "work/oc", false);
        let found: Vec<&str> = hits.iter().map(|hit| hit.name.as_str()).collect();
        assert_eq!(found, ["octo", "octopus"], "a slash names the folder by its last part and its parents by the rest");
        assert!(score("octo", "collections").is_none(), "letters in order must start a word");
        for dir in ["a/octo-one", "b/octo-two", "c/o-c-t-o"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let (hits, _) = search(&root, "octo", false);
        assert!(
            hits.iter().all(|hit| hit.name.contains("octo")),
            "with enough whole matches, letters in order drop out"
        );
        let (hits, _) = search(&root, "octo", true);
        assert!(hits.iter().any(|hit| hit.rel == ".hidden"), "hidden folders only when asked");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_listing_hides_dot_folders_unless_asked_and_marks_repositories() {
        let root = temp("list");
        for dir in ["beta/.git", "Alpha", ".config"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let names = |listing: Listing| listing.dirs.into_iter().map(|(name, _, git)| (name, git)).collect::<Vec<_>>();
        assert_eq!(names(list(&root, false)), [("Alpha".into(), false), ("beta".into(), true)]);
        assert_eq!(names(list(&root, true)).len(), 3);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn places_start_at_home_and_never_repeat_a_folder() {
        let places = places();
        assert_eq!(places.first().map(|place| place.kind), dirs::home_dir().map(|_| "home"));
        let mut paths: Vec<_> = places.iter().map(|place| &place.path).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), places.len());
    }
}
