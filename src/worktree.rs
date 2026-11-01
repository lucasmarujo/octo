use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::fsutil;
use crate::launch;
use crate::model::{AgentId, AgentRecord, Worktree};
use crate::paths::{Home, create_private_dir, normalize_for_compare};
use crate::project;

pub const BRANCH_PREFIX: &str = "orkai/";
pub const MAX_UNTRACKED_FILES: usize = 200;
pub const MAX_COUNTED_BYTES: u64 = 1024 * 1024;
pub const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;
pub const CHANGES_EVERY_MS: u64 = 10_000;
pub const MAX_LOG_COMMITS: usize = 20;
pub const MAX_COMMIT_FILES: usize = 50;
const MIN_HASH_LEN: usize = 4;
const MAX_HASH_LEN: usize = 64;
const REMOTE_REFS: &str = "refs/remotes";
const PREFERRED_REMOTE: &str = "origin";
const AZURE_SSH_HOST: &str = "ssh.dev.azure.com";
const AZURE_HOST: &str = "dev.azure.com";
const AZURE_SSH_VERSION: &str = "v3/";
const AZURE_REPO_SEGMENT: &str = "_git";
const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%an%x1f%ct%x1f%s";
const LOG_FIELD: char = '\u{1f}';
const MS_PER_SECOND: u64 = 1000;
const BRANCH_SLUG_MAX: usize = 40;
const MAX_BRANCH_ATTEMPTS: usize = 100;
const DEFAULT_SLUG: &str = "agent";
const NULL_FILE: &str = "/dev/null";
const HEAD: &str = "HEAD";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub files: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub paths: BTreeSet<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.files == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub renamed_from: Option<String>,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overlap {
    pub agent: AgentId,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merged {
    pub target: String,
    pub tip: String,
    pub up_to_date: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub repo: PathBuf,
    pub branch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Committed {
    pub branch: String,
    pub tip: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub at_ms: u64,
    pub subject: String,
    pub on_branch: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitFile {
    pub status: char,
    pub path: String,
    pub renamed_from: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitDetail {
    pub files: Vec<CommitFile>,
    pub more: usize,
    pub url: Option<String>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn git(dir: &Path, args: &[&str]) -> io::Result<Output> {
    let program =
        which::which("git").map_err(|_| io::Error::new(io::ErrorKind::NotFound, "git was not found in PATH"))?;
    launch::without_console(&mut Command::new(program))
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
}

fn error_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    lines
        .iter()
        .find(|line| line.starts_with("fatal:") || line.starts_with("error:"))
        .or_else(|| lines.iter().find(|line| !line.starts_with("warning:") && !line.starts_with("hint:")))
        .map_or_else(|| "git failed".to_string(), |line| line.to_string())
}

fn run(dir: &Path, args: &[&str]) -> io::Result<String> {
    let output = git(dir, args)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
    } else {
        Err(io::Error::other(error_line(&output.stderr)))
    }
}

fn native(text: &str) -> PathBuf {
    if cfg!(windows) { PathBuf::from(text.replace('/', "\\")) } else { PathBuf::from(text) }
}

pub fn branch_slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.chars().take(BRANCH_SLUG_MAX).collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() { DEFAULT_SLUG.to_string() } else { slug.to_string() }
}

fn branch_exists(repo: &Path, branch: &str) -> io::Result<bool> {
    Ok(git(repo, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")])?.status.success())
}

fn free_branch(repo: &Path, name: &str) -> io::Result<String> {
    let base = format!("{BRANCH_PREFIX}{}", branch_slug(name));
    for attempt in 1..=MAX_BRANCH_ATTEMPTS {
        let candidate = if attempt == 1 { base.clone() } else { format!("{base}-{attempt}") };
        if !branch_exists(repo, &candidate)? {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("every branch name like {base} is taken")))
}

pub fn toplevel(dir: &Path) -> io::Result<PathBuf> {
    run(dir, &["rev-parse", "--show-toplevel"])
        .map(|toplevel| native(&toplevel))
        .map_err(|_| invalid(format!("{} is not inside a git repository", dir.display())))
}

pub fn create(home: &Home, id: AgentId, name: &str, source: &Path) -> io::Result<(Worktree, PathBuf)> {
    let repo = toplevel(source)?;
    let prefix = run(source, &["rev-parse", "--show-prefix"])?;
    let base = run(&repo, &["rev-parse", "--verify", "HEAD^{commit}"])
        .map_err(|_| invalid("the repository has no commits yet; commit once before starting agents in worktrees"))?;
    let branch = free_branch(&repo, name)?;
    create_private_dir(&home.worktrees_dir())?;
    let path = home.worktree_dir(id);
    if path.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already exists", path.display())));
    }
    run(&repo, &["worktree", "add", "-b", &branch, &path.to_string_lossy(), &base])?;
    if let Ok(Some(loaded)) = project::load(&repo) {
        project::copy_files(&ignored(&repo, &loaded.config.copy), &repo, &path);
    }
    let relative: PathBuf = prefix.split('/').filter(|part| !part.is_empty()).collect();
    let cwd = Some(path.join(relative)).filter(|dir| dir.is_dir()).unwrap_or_else(|| path.clone());
    Ok((Worktree { repo, path, branch, base, source: source.to_path_buf() }, cwd))
}

fn ignored(repo: &Path, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| git(repo, &["check-ignore", "-q", "--", path]).is_ok_and(|output| output.status.success()))
        .cloned()
        .collect()
}

fn untracked(path: &Path) -> io::Result<Vec<String>> {
    let listing = run(path, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    Ok(listing.split('\0').filter(|name| !name.is_empty()).map(str::to_string).collect())
}

fn count_lines(path: &Path) -> usize {
    let Ok(file) = fs::File::open(path) else { return 0 };
    let mut data = Vec::new();
    if file.take(MAX_COUNTED_BYTES).read_to_end(&mut data).is_err() || data.contains(&0) {
        return 0;
    }
    data.iter().filter(|b| **b == b'\n').count() + usize::from(data.last().is_some_and(|b| *b != b'\n'))
}

pub fn files(worktree: &Worktree) -> io::Result<Vec<FileChange>> {
    files_since(&worktree.path, &worktree.base)
}

pub fn pending(folder: &Folder) -> io::Result<Vec<FileChange>> {
    files_since(&folder.repo, HEAD)
}

fn files_since(path: &Path, base: &str) -> io::Result<Vec<FileChange>> {
    let numstat = run(path, &["diff", "--numstat", "-z", base, "--"])?;
    let mut files = Vec::new();
    let mut fields = numstat.split('\0');
    while let Some(field) = fields.next() {
        let mut parts = field.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let (path, renamed_from) = if path.is_empty() {
            let from = fields.next().filter(|p| !p.is_empty()).map(str::to_string);
            let to = fields.next().filter(|p| !p.is_empty()).map(str::to_string);
            match (to, from) {
                (Some(to), from) => (to, from),
                (None, Some(from)) => (from, None),
                (None, None) => continue,
            }
        } else {
            (path.to_string(), None)
        };
        files.push(FileChange {
            path,
            renamed_from,
            insertions: added.parse::<usize>().unwrap_or(0),
            deletions: removed.parse::<usize>().unwrap_or(0),
        });
    }
    for (index, file) in untracked(path)?.into_iter().enumerate() {
        let insertions = if index < MAX_UNTRACKED_FILES { count_lines(&path.join(&file)) } else { 0 };
        files.push(FileChange { path: file, renamed_from: None, insertions, deletions: 0 });
    }
    Ok(files)
}

pub fn changes(worktree: &Worktree) -> io::Result<Changes> {
    let mut changes = Changes::default();
    for file in files(worktree)? {
        changes.files += 1;
        changes.insertions += file.insertions;
        changes.deletions += file.deletions;
        changes.paths.extend(file.renamed_from);
        changes.paths.insert(file.path);
    }
    Ok(changes)
}

pub fn diff(worktree: &Worktree) -> io::Result<Vec<u8>> {
    diff_since(&worktree.path, &worktree.base)
}

pub fn pending_diff(folder: &Folder) -> io::Result<Vec<u8>> {
    diff_since(&folder.repo, HEAD)
}

fn diff_since(path: &Path, base: &str) -> io::Result<Vec<u8>> {
    let output = git(path, &["diff", "--color=always", "--stat", "--patch", base, "--"])?;
    if !output.status.success() {
        return Err(io::Error::other(error_line(&output.stderr)));
    }
    let mut out = output.stdout;
    let new_files = untracked(path)?;
    for file in new_files.iter().take(MAX_UNTRACKED_FILES) {
        if out.len() >= MAX_DIFF_BYTES {
            break;
        }
        let added = git(path, &["diff", "--no-index", "--color=always", "--", NULL_FILE, file])?;
        out.extend_from_slice(&added.stdout);
    }
    if new_files.len() > MAX_UNTRACKED_FILES {
        out.extend_from_slice(
            format!("\n{} more new files are not shown\n", new_files.len() - MAX_UNTRACKED_FILES).as_bytes(),
        );
    }
    if out.len() > MAX_DIFF_BYTES {
        out.truncate(MAX_DIFF_BYTES);
        out.extend_from_slice(b"\x1b[m\n(diff truncated)\n");
    }
    Ok(out)
}

pub fn summary(worktree: &Worktree) -> io::Result<Vec<u8>> {
    let output = git(&worktree.path, &["diff", "--color=always", "--stat", &worktree.base])?;
    if !output.status.success() {
        return Err(io::Error::other(error_line(&output.stderr)));
    }
    let mut out = output.stdout;
    let new_files = untracked(&worktree.path)?;
    for file in new_files.iter().take(MAX_UNTRACKED_FILES) {
        let lines = count_lines(&worktree.path.join(file));
        out.extend_from_slice(format!(" {file} (new, {lines} lines)\n").as_bytes());
    }
    if new_files.len() > MAX_UNTRACKED_FILES {
        out.extend_from_slice(format!(" {} more new files\n", new_files.len() - MAX_UNTRACKED_FILES).as_bytes());
    }
    Ok(out)
}

pub fn merge(worktree: &Worktree, message: &str) -> io::Result<Merged> {
    if !run(&worktree.path, &["status", "--porcelain"])?.is_empty() {
        run(&worktree.path, &["add", "-A"])?;
        run(&worktree.path, &["commit", "-m", message])
            .map_err(|err| io::Error::other(format!("could not commit the worktree changes: {err}")))?;
    }
    let tip = run(&worktree.path, &["rev-parse", "HEAD"])?;
    let target = current_branch(&worktree.repo)?;
    let before = run(&worktree.repo, &["rev-parse", "HEAD"])?;
    let output = git(&worktree.repo, &["merge", "--no-edit", &worktree.branch])?;
    if output.status.success() {
        let after = run(&worktree.repo, &["rev-parse", "HEAD"])?;
        return Ok(Merged { target, tip, up_to_date: before == after });
    }
    let conflicts = run(&worktree.repo, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
    if conflicts.is_empty() {
        return Err(io::Error::other(error_line(&output.stderr)));
    }
    let _ = run(&worktree.repo, &["merge", "--abort"]);
    Err(io::Error::other(format!(
        "{} file(s) conflict with {target}, so the merge was undone; run `git merge {}` in {} to resolve them",
        conflicts.lines().count(),
        worktree.branch,
        worktree.repo.display()
    )))
}

fn current_branch(repo: &Path) -> io::Result<String> {
    run(repo, &["symbolic-ref", "--short", "-q", HEAD])
        .map_err(|_| invalid(format!("{} is not on a branch", repo.display())))
}

pub fn folder(dir: &Path) -> io::Result<Folder> {
    let repo = toplevel(dir)?;
    let branch = current_branch(&repo).unwrap_or_else(|_| HEAD.to_string());
    Ok(Folder { repo, branch })
}

pub fn log(dir: &Path, base: Option<&str>) -> io::Result<Vec<Commit>> {
    let limit = format!("--max-count={MAX_LOG_COMMITS}");
    let listed = run(dir, &["log", &limit, "-z", LOG_FORMAT, HEAD, "--"])?;
    let on_branch: HashSet<String> = match base {
        Some(base) => {
            run(dir, &["rev-list", &limit, &format!("{base}..{HEAD}"), "--"])?.lines().map(str::to_string).collect()
        }
        None => HashSet::new(),
    };
    Ok(listed.split('\0').filter_map(|record| parse_commit(record, &on_branch)).collect())
}

fn parse_commit(record: &str, on_branch: &HashSet<String>) -> Option<Commit> {
    let mut fields = record.trim_start_matches('\n').splitn(5, LOG_FIELD);
    let (full, hash, author, at, subject) =
        (fields.next()?, fields.next()?, fields.next()?, fields.next()?, fields.next()?);
    Some(Commit {
        hash: hash.to_string(),
        author: author.to_string(),
        at_ms: at.parse::<u64>().ok()?.saturating_mul(MS_PER_SECOND),
        subject: subject.to_string(),
        on_branch: on_branch.contains(full),
    })
}

fn is_commit_hash(text: &str) -> bool {
    (MIN_HASH_LEN..=MAX_HASH_LEN).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn commit_detail(dir: &Path, hash: &str) -> io::Result<CommitDetail> {
    if !is_commit_hash(hash) {
        return Err(invalid(format!("{hash} is not a commit hash")));
    }
    let shown = run(
        dir,
        &["show", "--no-color", "--format=%H", "--name-status", "-z", "-M", "--diff-merges=first-parent", hash, "--"],
    )?;
    let mut fields = shown.split('\0');
    let full = fields.next().unwrap_or_default().trim().to_string();
    let mut detail = CommitDetail::default();
    while let Some(status) = fields.next() {
        let Some(letter) = status.trim_start().chars().next() else { continue };
        let renamed_from = matches!(letter, 'R' | 'C').then(|| fields.next().map(str::to_string)).flatten();
        let Some(path) = fields.next() else { break };
        if detail.files.len() < MAX_COMMIT_FILES {
            detail.files.push(CommitFile { status: letter, path: path.to_string(), renamed_from });
        } else {
            detail.more += 1;
        }
    }
    detail.url = remote_holding(dir, &full)?.and_then(|remote| web_commit_url(&remote, &full));
    Ok(detail)
}

fn remote_holding(dir: &Path, hash: &str) -> io::Result<Option<String>> {
    let holding = run(dir, &["for-each-ref", &format!("--contains={hash}"), "--format=%(refname)", REMOTE_REFS])?;
    if holding.is_empty() {
        return Ok(None);
    }
    let configured = git(dir, &["config", "--get-regexp", r"^remote\..*\.url$"])?;
    let remotes: Vec<(String, String)> = String::from_utf8_lossy(&configured.stdout)
        .lines()
        .filter_map(|line| {
            let (key, url) = line.split_once(' ')?;
            let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
            Some((name.to_string(), url.trim().to_string()))
        })
        .collect();
    let holds = |name: &str| holding.lines().any(|reference| reference.starts_with(&format!("{REMOTE_REFS}/{name}/")));
    let holder = remotes
        .iter()
        .find(|(name, _)| name == PREFERRED_REMOTE && holds(name))
        .or_else(|| remotes.iter().find(|(name, _)| holds(name)));
    Ok(holder.map(|(_, url)| url.clone()))
}

pub fn web_commit_url(remote: &str, hash: &str) -> Option<String> {
    let (scheme, rest, keeps_port) = if let Some((scheme, rest)) = remote.split_once("://") {
        match scheme {
            "https" | "http" => (scheme, rest, true),
            "ssh" | "git" => ("https", rest, false),
            _ => return None,
        }
    } else {
        ("https", remote, false)
    };
    let rest = match rest.split_once('@') {
        Some((user, after)) if !user.contains('/') => after,
        _ => rest,
    };
    let (authority, path) = if remote.contains("://") {
        rest.split_once('/')?
    } else {
        let (host, path) = rest.split_once(':')?;
        if host.len() < 2 || host.contains(['/', '\\']) {
            return None;
        }
        (host, path)
    };
    let host = if keeps_port { authority } else { authority.split(':').next()? };
    let path = path.trim_matches('/').trim_end_matches(".git");
    if host.is_empty() || path.is_empty() || path.contains('\\') {
        return None;
    }
    let (host, path) = match path.strip_prefix(AZURE_SSH_VERSION) {
        Some(azure) if host == AZURE_SSH_HOST => {
            let mut parts = azure.splitn(3, '/');
            let (org, project, repo) = (parts.next()?, parts.next()?, parts.next()?);
            (AZURE_HOST, format!("{org}/{project}/{AZURE_REPO_SEGMENT}/{repo}"))
        }
        _ => (host, path.to_string()),
    };
    let commit = if host.contains("gitlab") {
        "-/commit"
    } else if host.contains("bitbucket") {
        "commits"
    } else {
        "commit"
    };
    Some(format!("{scheme}://{host}/{path}/{commit}/{hash}"))
}

pub fn commit(folder: &Folder, message: &str) -> io::Result<Option<Committed>> {
    let branch = current_branch(&folder.repo)?;
    if run(&folder.repo, &["status", "--porcelain"])?.is_empty() {
        return Ok(None);
    }
    run(&folder.repo, &["add", "-A"])?;
    run(&folder.repo, &["commit", "-m", message])?;
    let tip = run(&folder.repo, &["rev-parse", "--short", HEAD])?;
    Ok(Some(Committed { branch, tip }))
}

fn owned_by_orkai(home: &Home, worktree: &Worktree) -> bool {
    let root = normalize_for_compare(&home.worktrees_dir());
    normalize_for_compare(&worktree.path).starts_with(&format!("{root}/")) && worktree.branch.starts_with(BRANCH_PREFIX)
}

pub fn discard(home: &Home, worktree: &Worktree) -> io::Result<()> {
    if !owned_by_orkai(home, worktree) {
        return Err(invalid(format!("{} was not created by orkai", worktree.path.display())));
    }
    let removed = run(&worktree.repo, &["worktree", "remove", "--force", &worktree.path.to_string_lossy()]);
    if removed.is_err() && worktree.path.exists() {
        fsutil::remove_dir_all_retry(&worktree.path)?;
    }
    let _ = run(&worktree.repo, &["worktree", "prune"]);
    let _ = run(&worktree.repo, &["branch", "-D", &worktree.branch]);
    Ok(())
}

#[derive(Default)]
pub struct ChangeWatcher {
    scanned: HashMap<AgentId, u64>,
}

impl ChangeWatcher {
    pub fn scan(&mut self, targets: &[(AgentId, Worktree)], now: u64) -> BTreeMap<AgentId, Changes> {
        self.scanned.retain(|id, _| targets.iter().any(|(target, _)| target == id));
        let mut found = BTreeMap::new();
        for (id, worktree) in targets {
            if self.scanned.get(id).is_some_and(|at| now.saturating_sub(*at) < CHANGES_EVERY_MS) {
                continue;
            }
            self.scanned.insert(*id, now);
            if let Ok(changes) = changes(worktree) {
                found.insert(*id, changes);
            }
        }
        found
    }
}

pub fn project_of(cwd: &Path, records: &[AgentRecord]) -> Option<PathBuf> {
    let cwd = normalize_for_compare(cwd);
    records
        .iter()
        .filter_map(|record| record.worktree.as_ref())
        .find(|worktree| {
            let root = normalize_for_compare(&worktree.path);
            cwd == root || cwd.starts_with(&format!("{root}/"))
        })
        .map(|worktree| worktree.source.clone())
}

pub fn overlaps(changed: &[(AgentId, &Worktree, &Changes)]) -> BTreeMap<AgentId, Vec<Overlap>> {
    let mut by_repo: BTreeMap<String, Vec<(AgentId, &Changes)>> = BTreeMap::new();
    for (id, worktree, changes) in changed {
        by_repo.entry(normalize_for_compare(&worktree.repo)).or_default().push((*id, *changes));
    }
    let mut found: BTreeMap<AgentId, Vec<Overlap>> = BTreeMap::new();
    for agents in by_repo.values() {
        for (index, (first, first_changes)) in agents.iter().enumerate() {
            for (second, second_changes) in &agents[index + 1..] {
                let paths: Vec<String> = first_changes.paths.intersection(&second_changes.paths).cloned().collect();
                if paths.is_empty() {
                    continue;
                }
                found.entry(*first).or_default().push(Overlap { agent: *second, paths: paths.clone() });
                found.entry(*second).or_default().push(Overlap { agent: *first, paths });
            }
        }
    }
    for list in found.values_mut() {
        list.sort_by_key(|overlap| overlap.agent);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Repo {
        dir: PathBuf,
        home: Home,
        repo: PathBuf,
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn paths(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|path| path.to_string()).collect()
    }

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let output = git(dir, args).unwrap();
        assert!(output.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn repo() -> Repo {
        let dir = std::env::temp_dir().join(format!(
            "orkai-worktree-{}-{}-{}",
            std::process::id(),
            now_ms(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let repo = dir.join("project");
        fs::create_dir_all(repo.join("app")).unwrap();
        let home = Home::at(dir.join("home"));
        home.ensure().unwrap();
        git_in(&repo, &["init", "-q", "-b", "main"]);
        git_in(&repo, &["config", "user.email", "orkai@example.com"]);
        git_in(&repo, &["config", "user.name", "orkai"]);
        git_in(&repo, &["config", "core.autocrlf", "false"]);
        fs::write(repo.join("app").join("lib.txt"), "one\ntwo\nthree\n").unwrap();
        fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        git_in(&repo, &["add", "."]);
        git_in(&repo, &["commit", "-q", "-m", "init"]);
        Repo { dir, home, repo }
    }

    #[test]
    fn branch_names_are_safe_slugs() {
        assert_eq!(branch_slug("Fix Login Bug"), "fix-login-bug");
        assert_eq!(branch_slug("  ??weird//name..lock  "), "weird-name-lock");
        assert_eq!(branch_slug("çãõ"), DEFAULT_SLUG);
        assert_eq!(branch_slug(&"a".repeat(80)).len(), BRANCH_SLUG_MAX);
    }

    #[test]
    fn an_agent_gets_its_own_branch_and_folder_from_the_current_commit() {
        let r = repo();
        let source = r.repo.join("app");
        let (worktree, cwd) = create(&r.home, 7, "Fix Login", &source).unwrap();
        assert_eq!(worktree.branch, "orkai/fix-login");
        assert_eq!(worktree.path, r.home.worktree_dir(7));
        assert_eq!(worktree.source, source);
        assert_eq!(worktree.base, git_in(&r.repo, &["rev-parse", "HEAD"]));
        assert!(worktree.repo.join("app").join("lib.txt").is_file());
        assert_eq!(git_in(&worktree.repo, &["branch", "--show-current"]), "main");
        assert_eq!(cwd, worktree.path.join("app"));
        assert!(cwd.join("lib.txt").is_file());
        assert_eq!(git_in(&cwd, &["branch", "--show-current"]), "orkai/fix-login");

        let (second, root) = create(&r.home, 8, "Fix Login", &r.repo).unwrap();
        assert_eq!(second.branch, "orkai/fix-login-2");
        assert_eq!(root, second.path);
        let nested = r.repo.join("app").join("deep");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("keep.txt"), "x\n").unwrap();
        git_in(&r.repo, &["add", "."]);
        git_in(&r.repo, &["commit", "-q", "-m", "deep"]);
        let (_, deep) = create(&r.home, 9, "Deep", &nested).unwrap();
        assert_eq!(deep, r.home.worktree_dir(9).join("app").join("deep"));
        assert_eq!(changes(&worktree).unwrap(), Changes::default());
    }

    fn subjects(commits: &[Commit]) -> Vec<&str> {
        commits.iter().map(|commit| commit.subject.as_str()).collect()
    }

    #[test]
    fn the_log_lists_the_latest_commits_like_git_log_and_marks_the_ones_on_the_agent_branch() {
        let r = repo();
        git_in(&r.repo, &["commit", "-q", "--allow-empty", "-m", "second on main: tabs | panes"]);
        let (worktree, _) = create(&r.home, 3, "Feature", &r.repo).unwrap();
        git_in(&worktree.path, &["commit", "-q", "--allow-empty", "-m", "feature work"]);
        git_in(&r.repo, &["commit", "-q", "--allow-empty", "-m", "main moved on"]);

        let commits = log(&worktree.path, Some(&worktree.base)).unwrap();
        assert_eq!(subjects(&commits), ["feature work", "second on main: tabs | panes", "init"]);
        assert_eq!(commits.iter().map(|commit| commit.on_branch).collect::<Vec<_>>(), [true, false, false]);
        assert_eq!(commits[0].hash, git_in(&worktree.path, &["rev-parse", "--short", "HEAD"]));
        assert_eq!(commits[0].author, "orkai");
        let committed_at = git_in(&worktree.path, &["log", "-1", "--format=%ct"]).parse::<u64>().unwrap() * 1000;
        assert_eq!(commits[0].at_ms, committed_at);

        let main = log(&folder(&r.repo.join("app")).unwrap().repo, None).unwrap();
        assert_eq!(subjects(&main), ["main moved on", "second on main: tabs | panes", "init"]);
        assert!(main.iter().all(|commit| !commit.on_branch), "an agent outside a worktree has no branch of its own");

        for index in 0..MAX_LOG_COMMITS {
            git_in(&r.repo, &["commit", "-q", "--allow-empty", "-m", &format!("more {index}")]);
        }
        let latest = log(&r.repo, None).unwrap();
        assert_eq!(latest.len(), MAX_LOG_COMMITS, "only the latest commits are read");
        assert_eq!(latest[0].subject, format!("more {}", MAX_LOG_COMMITS - 1));

        let empty = r.dir.join("empty-log");
        fs::create_dir_all(&empty).unwrap();
        git_in(&empty, &["init", "-q"]);
        assert!(log(&empty, None).is_err(), "a repository without commits has no log");
    }

    #[test]
    fn folders_outside_git_or_without_commits_are_refused() {
        let r = repo();
        let plain = r.dir.join("plain");
        fs::create_dir_all(&plain).unwrap();
        let err = create(&r.home, 1, "x", &plain).unwrap_err();
        assert!(err.to_string().contains("is not inside a git repository"), "{err}");
        let empty = r.dir.join("empty");
        fs::create_dir_all(&empty).unwrap();
        git_in(&empty, &["init", "-q"]);
        let err = create(&r.home, 2, "x", &empty).unwrap_err();
        assert!(err.to_string().contains("no commits yet"), "{err}");
        assert!(!r.home.worktree_dir(2).exists());
    }

    #[test]
    fn changes_count_edits_commits_and_new_files_but_not_ignored_ones() {
        let r = repo();
        let (worktree, cwd) = create(&r.home, 1, "edit", &r.repo.join("app")).unwrap();
        fs::write(cwd.join("lib.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        fs::write(cwd.join("new.txt"), "a\nb\nc").unwrap();
        fs::create_dir_all(worktree.path.join("target")).unwrap();
        fs::write(worktree.path.join("target").join("build.log"), "ignored\n").unwrap();
        let expected = Changes { files: 2, insertions: 5, deletions: 1, paths: paths(&["app/lib.txt", "app/new.txt"]) };
        assert_eq!(changes(&worktree).unwrap(), expected);
        let listed: Vec<(String, usize, usize)> =
            files(&worktree).unwrap().into_iter().map(|f| (f.path, f.insertions, f.deletions)).collect();
        assert_eq!(listed, [("app/lib.txt".to_string(), 2, 1), ("app/new.txt".to_string(), 3, 0)], "one line per file");

        git_in(&cwd, &["add", "new.txt"]);
        git_in(&cwd, &["commit", "-q", "-m", "add"]);
        assert_eq!(changes(&worktree).unwrap(), expected);

        let text = String::from_utf8_lossy(&diff(&worktree).unwrap()).into_owned();
        assert!(text.contains("lib.txt") && text.contains("new.txt") && text.contains("TWO"), "{text}");
        assert!(!text.contains("build.log"), "{text}");
    }

    #[test]
    fn the_diff_includes_files_the_agent_has_not_added_yet() {
        let r = repo();
        let (worktree, cwd) = create(&r.home, 1, "new", &r.repo).unwrap();
        fs::write(cwd.join("draft.md"), "# Plan\n").unwrap();
        let text = String::from_utf8_lossy(&diff(&worktree).unwrap()).into_owned();
        assert!(text.contains("draft.md") && text.contains("# Plan"), "{text}");
        assert!(diff(&create(&r.home, 2, "clean", &r.repo).unwrap().0).unwrap().is_empty());
    }

    #[test]
    fn merging_commits_the_pending_work_and_brings_it_to_the_current_branch() {
        let r = repo();
        let (worktree, cwd) = create(&r.home, 1, "feature", &r.repo).unwrap();
        fs::write(cwd.join("app").join("lib.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        fs::write(cwd.join("notes.txt"), "hello\n").unwrap();
        let merged = merge(&worktree, "feature: add notes").unwrap();
        assert_eq!(merged.target, "main");
        assert!(!merged.up_to_date);
        assert_eq!(merged.tip, git_in(&cwd, &["rev-parse", "HEAD"]));
        assert_eq!(git_in(&cwd, &["log", "-1", "--format=%s"]), "feature: add notes");
        assert_eq!(fs::read_to_string(r.repo.join("notes.txt")).unwrap(), "hello\n");
        assert!(git_in(&r.repo, &["status", "--porcelain"]).is_empty());

        let again = merge(&worktree, "feature: nothing").unwrap();
        assert!(again.up_to_date);
    }

    #[test]
    fn a_conflicting_merge_is_undone_and_explained() {
        let r = repo();
        let (worktree, cwd) = create(&r.home, 1, "clash", &r.repo).unwrap();
        fs::write(cwd.join("app").join("lib.txt"), "agent\n").unwrap();
        fs::write(r.repo.join("app").join("lib.txt"), "human\n").unwrap();
        git_in(&r.repo, &["commit", "-q", "-am", "human edit"]);
        let err = merge(&worktree, "clash").unwrap_err().to_string();
        assert!(err.contains("1 file(s) conflict with main") && err.contains("git merge orkai/clash"), "{err}");
        assert!(git_in(&r.repo, &["status", "--porcelain"]).is_empty(), "the repository must be left as it was");
        assert_eq!(fs::read_to_string(r.repo.join("app").join("lib.txt")).unwrap(), "human\n");
    }

    #[test]
    fn merging_needs_the_repository_on_a_branch() {
        let r = repo();
        let (worktree, _) = create(&r.home, 1, "detached", &r.repo).unwrap();
        let head = git_in(&r.repo, &["rev-parse", "HEAD"]);
        git_in(&r.repo, &["checkout", "-q", "--detach", &head]);
        let err = merge(&worktree, "x").unwrap_err().to_string();
        assert!(err.contains("is not on a branch"), "{err}");
    }

    fn listed(files: Vec<FileChange>) -> Vec<(String, usize, usize)> {
        let mut listed: Vec<_> = files.into_iter().map(|file| (file.path, file.insertions, file.deletions)).collect();
        listed.sort();
        listed
    }

    #[test]
    fn a_folder_lists_what_is_pending_since_its_last_commit_but_not_ignored_files() {
        let r = repo();
        let folder = folder(&r.repo.join("app")).unwrap();
        assert_eq!(folder, Folder { repo: toplevel(&r.repo).unwrap(), branch: "main".into() });
        assert!(pending(&folder).unwrap().is_empty());
        assert!(pending_diff(&folder).unwrap().is_empty());

        fs::write(r.repo.join("app").join("lib.txt"), "one\nTWO\nthree\n").unwrap();
        fs::write(r.repo.join("notes.txt"), "a\nb\n").unwrap();
        fs::write(r.repo.join(HEAD), "named like the revision\n").unwrap();
        fs::create_dir_all(r.repo.join("target")).unwrap();
        fs::write(r.repo.join("target").join("build.log"), "ignored\n").unwrap();
        let expected = [(HEAD.to_string(), 1, 0), ("app/lib.txt".to_string(), 1, 1), ("notes.txt".to_string(), 2, 0)];
        assert_eq!(listed(pending(&folder).unwrap()), expected);

        git_in(&r.repo, &["add", "notes.txt", HEAD]);
        assert_eq!(listed(pending(&folder).unwrap()), expected, "a staged file is still pending");
        let text = String::from_utf8_lossy(&pending_diff(&folder).unwrap()).into_owned();
        assert!(text.contains("app/lib.txt") && text.contains("TWO") && text.contains("notes.txt"), "{text}");
        assert!(!text.contains("build.log"), "{text}");
    }

    #[test]
    fn committing_a_folder_records_everything_pending_on_its_branch() {
        let r = repo();
        let folder = folder(&r.repo).unwrap();
        let before = git_in(&r.repo, &["rev-parse", "HEAD"]);
        assert_eq!(commit(&folder, "nothing").unwrap(), None, "a clean folder has nothing to commit");
        assert_eq!(git_in(&r.repo, &["rev-parse", "HEAD"]), before);

        fs::write(r.repo.join("app").join("lib.txt"), "one\n").unwrap();
        fs::write(r.repo.join("plan.md"), "# Plan\n").unwrap();
        let done = commit(&folder, "planner: write the plan").unwrap().unwrap();
        assert_eq!(done, Committed { branch: "main".into(), tip: git_in(&r.repo, &["rev-parse", "--short", "HEAD"]) });
        assert_eq!(git_in(&r.repo, &["log", "-1", "--format=%s"]), "planner: write the plan");
        assert_eq!(git_in(&r.repo, &["show", "--name-only", "--format=", "HEAD"]), "app/lib.txt\nplan.md");
        assert!(git_in(&r.repo, &["status", "--porcelain"]).is_empty());
        assert!(pending(&folder).unwrap().is_empty());
    }

    #[test]
    fn a_folder_off_any_branch_shows_its_changes_but_is_never_committed() {
        let r = repo();
        let head = git_in(&r.repo, &["rev-parse", "HEAD"]);
        git_in(&r.repo, &["checkout", "-q", "--detach", &head]);
        fs::write(r.repo.join("loose.txt"), "x\n").unwrap();
        let folder = folder(&r.repo).unwrap();
        assert_eq!(folder.branch, HEAD);
        assert_eq!(listed(pending(&folder).unwrap()), [("loose.txt".to_string(), 1, 0)]);
        let err = commit(&folder, "x").unwrap_err().to_string();
        assert!(err.contains("is not on a branch"), "{err}");
        assert_eq!(git_in(&r.repo, &["rev-parse", "HEAD"]), head);
        assert!(r.repo.join("loose.txt").is_file());
    }

    #[test]
    fn a_folder_outside_git_has_no_changes_to_read() {
        let r = repo();
        let plain = r.dir.join("plain");
        fs::create_dir_all(&plain).unwrap();
        let err = folder(&plain).unwrap_err().to_string();
        assert!(err.contains("is not inside a git repository"), "{err}");
    }

    #[test]
    fn discarding_removes_the_folder_and_the_branch_but_only_ones_orkai_made() {
        let r = repo();
        let (worktree, cwd) = create(&r.home, 1, "throwaway", &r.repo).unwrap();
        fs::write(cwd.join("scratch.txt"), "x").unwrap();
        discard(&r.home, &worktree).unwrap();
        assert!(!worktree.path.exists());
        assert!(!branch_exists(&r.repo, "orkai/throwaway").unwrap());
        assert!(!git_in(&r.repo, &["worktree", "list"]).contains("throwaway"));

        let foreign = Worktree { path: r.repo.clone(), branch: "main".into(), ..worktree.clone() };
        assert!(discard(&r.home, &foreign).is_err());
        assert!(r.repo.join("app").join("lib.txt").exists());
        let user_branch = Worktree { branch: "main".into(), ..worktree };
        assert!(discard(&r.home, &user_branch).is_err());
        assert!(branch_exists(&r.repo, "main").unwrap());
    }

    #[test]
    fn a_worktree_deleted_by_hand_is_still_discarded_cleanly() {
        let r = repo();
        let (worktree, _) = create(&r.home, 1, "gone", &r.repo).unwrap();
        fs::remove_dir_all(&worktree.path).unwrap();
        discard(&r.home, &worktree).unwrap();
        assert!(!branch_exists(&r.repo, "orkai/gone").unwrap());
        assert!(!git_in(&r.repo, &["worktree", "list"]).contains("gone"));
    }

    #[test]
    fn the_watcher_rescans_each_worktree_at_most_once_per_interval() {
        let r = repo();
        let (worktree, cwd) = create(&r.home, 1, "watch", &r.repo).unwrap();
        let targets = vec![(1, worktree)];
        let mut watcher = ChangeWatcher::default();
        assert_eq!(watcher.scan(&targets, 1_000).get(&1), Some(&Changes::default()));
        fs::write(cwd.join("more.txt"), "1\n2\n").unwrap();
        assert!(watcher.scan(&targets, 1_000 + CHANGES_EVERY_MS - 1).is_empty());
        let later = watcher.scan(&targets, 1_000 + CHANGES_EVERY_MS);
        assert_eq!(
            later.get(&1),
            Some(&Changes { files: 1, insertions: 2, deletions: 0, paths: paths(&["more.txt"]) })
        );
        assert!(watcher.scan(&[], 50_000).is_empty());
        assert!(watcher.scanned.is_empty());
    }

    #[test]
    fn new_worktrees_get_the_ignored_files_the_project_lists_to_copy() {
        let r = repo();
        fs::write(r.repo.join(".gitignore"), "target/\n.env\n").unwrap();
        git_in(&r.repo, &["commit", "-q", "-am", "ignore env"]);
        fs::write(r.repo.join(".env"), "KEY=1\n").unwrap();
        fs::write(r.repo.join("notes.txt"), "not ignored\n").unwrap();
        fs::write(r.repo.join(project::FILE), r#"{"copy":[".env","notes.txt","app/lib.txt"]}"#).unwrap();
        let (worktree, _) = create(&r.home, 1, "copy", &r.repo).unwrap();
        assert_eq!(fs::read_to_string(worktree.path.join(".env")).unwrap(), "KEY=1\n");
        assert!(!worktree.path.join("notes.txt").exists(), "a file git would commit is never copied");
        assert_eq!(fs::read_to_string(worktree.path.join("app").join("lib.txt")).unwrap(), "one\ntwo\nthree\n");
        assert!(changes(&worktree).unwrap().is_empty(), "copied files never show up as changes");
    }

    #[test]
    fn the_summary_lists_changed_and_new_files_without_their_content() {
        let r = repo();
        let (worktree, cwd) = create(&r.home, 1, "summary", &r.repo).unwrap();
        assert!(summary(&worktree).unwrap().is_empty());
        fs::write(cwd.join("app").join("lib.txt"), "one\nTWO\nthree\n").unwrap();
        fs::write(cwd.join("plan.md"), "a\nb\n").unwrap();
        let text = String::from_utf8_lossy(&summary(&worktree).unwrap()).into_owned();
        assert!(text.contains("app/lib.txt") && text.contains("1 file changed"), "{text}");
        assert!(text.contains(" plan.md (new, 2 lines)"), "{text}");
        assert!(!text.contains("TWO"), "{text}");
    }

    #[test]
    fn a_renamed_file_counts_under_its_old_and_new_names() {
        let r = repo();
        let (worktree, cwd) = create(&r.home, 1, "rename", &r.repo).unwrap();
        git_in(&cwd, &["mv", "app/lib.txt", "app/core.txt"]);
        fs::write(cwd.join("app").join("draft file.md"), "x\n").unwrap();
        let found = changes(&worktree).unwrap();
        assert_eq!(found.paths, paths(&["app/core.txt", "app/draft file.md", "app/lib.txt"]));
    }

    fn tree(repo: &str) -> Worktree {
        Worktree {
            repo: PathBuf::from(repo),
            path: PathBuf::from(format!("{repo}-wt")),
            branch: "orkai/x".into(),
            base: "abc".into(),
            source: PathBuf::from(repo),
        }
    }

    fn touched(list: &[&str]) -> Changes {
        Changes { files: list.len(), paths: paths(list), ..Changes::default() }
    }

    #[test]
    fn a_folder_inside_an_agents_worktree_belongs_to_that_agents_project() {
        let record = |id: AgentId, worktree: Option<Worktree>| AgentRecord {
            id,
            session: "s".into(),
            name: format!("agent-{id}"),
            kind: crate::model::AgentKind::Claude,
            cwd: PathBuf::from("/elsewhere"),
            task: String::new(),
            created_at_ms: 1,
            conversation_id: None,
            launches: 0,
            worktree,
            bypass: false,
            remote_control: false,
            project: None,
            parent: None,
            race: None,
        };
        let web = Worktree { source: PathBuf::from("/src/web/app"), ..tree("/src/web") };
        let records = [record(1, None), record(2, Some(web))];
        let expected = Some(PathBuf::from("/src/web/app"));
        assert_eq!(project_of(Path::new("/src/web-wt"), &records), expected);
        assert_eq!(project_of(Path::new("/src/web-wt/app/deep"), &records), expected);
        assert_eq!(project_of(Path::new("/src/web-wt-2"), &records), None, "a sibling folder is not inside");
        assert_eq!(project_of(Path::new("/src/web"), &records), None);
        assert_eq!(project_of(Path::new("/elsewhere"), &records), None);
    }

    #[test]
    fn agents_of_the_same_repository_that_change_the_same_files_overlap() {
        let (web, api) = (tree("/src/web"), tree("/src/api"));
        let (a, b, c, d, e) = (
            touched(&["src/app.rs", "src/lib.rs", "README.md"]),
            touched(&["src/lib.rs", "src/app.rs"]),
            touched(&["docs/guide.md"]),
            touched(&["src/app.rs"]),
            touched(&["src/app.rs"]),
        );
        let changed = [(1, &web, &a), (2, &web, &b), (3, &web, &c), (4, &api, &d), (5, &web, &e)];
        let found = overlaps(&changed);
        let shared = vec!["src/app.rs".to_string(), "src/lib.rs".to_string()];
        assert_eq!(
            found[&1],
            vec![Overlap { agent: 2, paths: shared.clone() }, Overlap { agent: 5, paths: vec!["src/app.rs".into()] }]
        );
        assert_eq!(
            found[&2],
            vec![Overlap { agent: 1, paths: shared }, Overlap { agent: 5, paths: vec!["src/app.rs".into()] }]
        );
        assert_eq!(found[&5].iter().map(|o| o.agent).collect::<Vec<_>>(), [1, 2]);
        assert!(!found.contains_key(&3), "no shared file");
        assert!(!found.contains_key(&4), "another repository");
        assert!(overlaps(&[(1, &web, &a)]).is_empty());
        assert!(overlaps(&[]).is_empty());
    }

    fn file(status: char, path: &str, renamed_from: Option<&str>) -> CommitFile {
        CommitFile { status, path: path.into(), renamed_from: renamed_from.map(str::to_string) }
    }

    #[test]
    fn a_commit_lists_the_files_it_added_changed_renamed_and_deleted() {
        let r = repo();
        fs::write(r.repo.join("app").join("lib.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        fs::write(r.repo.join("app").join("new file.txt"), "new\n").unwrap();
        fs::remove_file(r.repo.join(".gitignore")).unwrap();
        git_in(&r.repo, &["add", "-A"]);
        git_in(&r.repo, &["commit", "-q", "-m", "change things"]);
        git_in(&r.repo, &["mv", "app/lib.txt", "app/library.txt"]);
        git_in(&r.repo, &["commit", "-q", "-m", "rename"]);

        let changed = commit_detail(&r.repo, &git_in(&r.repo, &["rev-parse", "--short", "HEAD~1"])).unwrap();
        assert_eq!(
            changed.files,
            [file('D', ".gitignore", None), file('M', "app/lib.txt", None), file('A', "app/new file.txt", None)]
        );
        assert_eq!(changed.more, 0);
        let renamed = commit_detail(&r.repo, &git_in(&r.repo, &["rev-parse", "--short", "HEAD"])).unwrap();
        assert_eq!(renamed.files, [file('R', "app/library.txt", Some("app/lib.txt"))]);
        let first = commit_detail(&r.repo, &git_in(&r.repo, &["rev-list", "--max-parents=0", "HEAD"])).unwrap();
        assert_eq!(
            first.files,
            [file('A', ".gitignore", None), file('A', "app/lib.txt", None)],
            "the first commit adds everything"
        );
        assert_eq!(first.url, None, "a repository without a remote has no web page for its commits");
    }

    #[test]
    fn a_commit_with_many_files_lists_the_first_ones_and_counts_the_rest() {
        let r = repo();
        for index in 0..MAX_COMMIT_FILES + 5 {
            fs::write(r.repo.join(format!("file-{index:03}.txt")), "x\n").unwrap();
        }
        git_in(&r.repo, &["add", "-A"]);
        git_in(&r.repo, &["commit", "-q", "-m", "many"]);
        let detail = commit_detail(&r.repo, "HEAD").unwrap_err();
        assert_eq!(detail.kind(), io::ErrorKind::InvalidInput, "only a commit hash is asked about");
        let detail = commit_detail(&r.repo, &git_in(&r.repo, &["rev-parse", "HEAD"])).unwrap();
        assert_eq!(detail.files.len(), MAX_COMMIT_FILES);
        assert_eq!(detail.more, 5);
    }

    #[test]
    fn a_commit_already_on_the_remote_opens_on_its_web_page_and_a_local_one_does_not() {
        let r = repo();
        let remote = r.dir.join("remote.git");
        git_in(&r.dir, &["init", "-q", "--bare", "remote.git"]);
        git_in(&r.repo, &["remote", "add", "origin", &remote.display().to_string()]);
        git_in(&r.repo, &["push", "-q", "origin", "main"]);
        let pushed = git_in(&r.repo, &["rev-parse", "HEAD"]);
        git_in(&r.repo, &["commit", "-q", "--allow-empty", "-m", "only here"]);
        let local = git_in(&r.repo, &["rev-parse", "--short", "HEAD"]);
        git_in(&r.repo, &["remote", "set-url", "origin", "https://user:secret@github.com/acme/app.git"]);

        let on_remote = commit_detail(&r.repo, &pushed[..7]).unwrap();
        assert_eq!(on_remote.url.as_deref(), Some(format!("https://github.com/acme/app/commit/{pushed}").as_str()));
        assert_eq!(commit_detail(&r.repo, &local).unwrap().url, None, "a commit nobody pushed only has its files");
    }

    #[test]
    fn the_remote_of_every_common_host_becomes_the_web_page_of_the_commit() {
        let hash = "0185de414b15da2aea478e6a39b8cd535a2be274";
        let cases = [
            ("https://github.com/acme/app.git", "https://github.com/acme/app/commit/"),
            ("https://user:token@github.com/acme/app", "https://github.com/acme/app/commit/"),
            ("git@github.com:acme/app.git", "https://github.com/acme/app/commit/"),
            ("ssh://git@github.com/acme/app.git", "https://github.com/acme/app/commit/"),
            ("ssh://git@git.acme.dev:2222/team/app.git", "https://git.acme.dev/team/app/commit/"),
            ("http://git.local/team/app.git", "http://git.local/team/app/commit/"),
            ("git://git.acme.dev/team/app.git", "https://git.acme.dev/team/app/commit/"),
            ("https://gitlab.com/group/sub/app.git", "https://gitlab.com/group/sub/app/-/commit/"),
            ("git@gitlab.acme.dev:group/app.git", "https://gitlab.acme.dev/group/app/-/commit/"),
            ("git@bitbucket.org:team/app.git", "https://bitbucket.org/team/app/commits/"),
            ("https://acme@dev.azure.com/acme/Project/_git/app", "https://dev.azure.com/acme/Project/_git/app/commit/"),
            ("git@ssh.dev.azure.com:v3/acme/Project/app", "https://dev.azure.com/acme/Project/_git/app/commit/"),
            (
                "https://acme.visualstudio.com/Project/_git/app",
                "https://acme.visualstudio.com/Project/_git/app/commit/",
            ),
        ];
        for (remote, page) in cases {
            assert_eq!(web_commit_url(remote, hash), Some(format!("{page}{hash}")), "{remote}");
        }
        for remote in
            ["/srv/git/app.git", "C:\\repos\\app", "file:///srv/git/app.git", "../app.git", "https://github.com/"]
        {
            assert_eq!(web_commit_url(remote, hash), None, "{remote}");
        }
    }
}
