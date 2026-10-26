use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitCode};
use std::sync::LazyLock;
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hooks::parse_version;
use crate::util::to_hex;

pub const DEFAULT_BASE_URL: &str = "https://github.com/lucasmarujo/orkai-cli/releases/latest/download";
pub const BASE_URL_ENV: &str = "ORKAI_UPDATE_URL";
pub const DISABLE_ENV: &str = "ORKAI_NO_UPDATE";
pub const UPDATED_ENV: &str = "ORKAI_UPDATED";
pub const MANIFEST_NAME: &str = "manifest.json";
pub const BUILD_COMMIT: Option<&str> = option_env!("ORKAI_BUILD_COMMIT");
const CHECK_TIMEOUT: Duration = Duration::from_secs(4);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const SLOW_CHECK_NOTICE: Duration = Duration::from_millis(500);
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;
const OLD_MARKER: &str = ".old-";
const STAGING_MARKER: &str = ".update-";
const SHORT_COMMIT: usize = 7;

pub static VERSION: LazyLock<String> =
    LazyLock::new(|| format!("{} ({})", env!("CARGO_PKG_VERSION"), BUILD_COMMIT.map_or("dev", short_commit)));

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub commit: String,
    pub version: String,
    #[serde(default)]
    pub built_at: String,
    pub assets: BTreeMap<String, Asset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    UpToDate,
    Install { to: String, asset: Asset },
    NoAsset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    Updated { to: String },
    NoAsset,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub fn short_commit(commit: &str) -> &str {
    commit.get(..SHORT_COMMIT).unwrap_or(commit)
}

pub fn current_target() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("x86_64-pc-windows-msvc")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-musl")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("x86_64-apple-darwin")
    } else {
        None
    }
}

fn is_commit(value: &str) -> bool {
    (SHORT_COMMIT..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let manifest: Manifest = serde_json::from_slice(bytes).map_err(|e| invalid(format!("bad manifest: {e}")))?;
        if !is_commit(&manifest.commit) {
            return Err(invalid("manifest has an invalid commit"));
        }
        for asset in manifest.assets.values() {
            let valid_hash = asset.sha256.len() == 64 && asset.sha256.bytes().all(|b| b.is_ascii_hexdigit());
            if !is_file_name(&asset.name) || !valid_hash || asset.size > MAX_BINARY_BYTES {
                return Err(invalid(format!("manifest lists an invalid asset {:?}", asset.name)));
            }
        }
        Ok(manifest)
    }
}

pub fn plan(manifest: &Manifest, current: Option<&str>, version: &str, target: &str) -> Plan {
    let newer =
        parse_version(&manifest.version).zip(parse_version(version)).is_some_and(|(latest, running)| latest > running);
    if current.is_some() && !newer {
        return Plan::UpToDate;
    }
    match manifest.assets.get(target) {
        Some(asset) => Plan::Install { to: manifest.commit.clone(), asset: asset.clone() },
        None => Plan::NoAsset,
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(Sha256::digest(bytes).as_slice())
}

pub struct Source {
    base_url: String,
}

impl Source {
    pub fn from_env() -> Self {
        let base_url = std::env::var(BASE_URL_ENV).ok().filter(|v| !v.trim().is_empty());
        Self::at(base_url.as_deref().unwrap_or(DEFAULT_BASE_URL))
    }

    pub fn at(base_url: &str) -> Self {
        Self { base_url: base_url.trim_end_matches('/').to_string() }
    }

    fn fetch(&self, name: &str, timeout: Duration, limit: u64) -> io::Result<Vec<u8>> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .https_only(self.base_url.starts_with("https://"))
            .user_agent(format!("orkai/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let url = format!("{}/{name}", self.base_url);
        let mut response = agent.get(&url).call().map_err(|e| io::Error::other(format!("{url}: {e}")))?;
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| io::Error::other(format!("{url}: {e}")))
    }

    pub fn manifest(&self) -> io::Result<Manifest> {
        Manifest::parse(&self.fetch(MANIFEST_NAME, CHECK_TIMEOUT, MAX_MANIFEST_BYTES)?)
    }

    pub fn download(&self, asset: &Asset) -> io::Result<Vec<u8>> {
        let bytes = self.fetch(&asset.name, DOWNLOAD_TIMEOUT, MAX_BINARY_BYTES)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != asset.size {
            return Err(invalid(format!("{} has {} bytes, expected {}", asset.name, bytes.len(), asset.size)));
        }
        if !sha256_hex(&bytes).eq_ignore_ascii_case(&asset.sha256) {
            return Err(invalid(format!("checksum mismatch for {}", asset.name)));
        }
        Ok(bytes)
    }
}

fn file_name(exe: &Path) -> io::Result<String> {
    exe.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "executable path has no file name"))
}

pub fn install(exe: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = exe.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "executable has no parent"))?;
    let name = file_name(exe)?;
    let pid = std::process::id();
    let staged = dir.join(format!(".{name}{STAGING_MARKER}{pid}"));
    let old = dir.join(format!("{name}{OLD_MARKER}{pid}"));
    let write = || -> io::Result<()> {
        let mut file = fs::File::create(&staged)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    };
    if let Err(err) = write() {
        let _ = fs::remove_file(&staged);
        return Err(err);
    }
    if let Err(err) = fs::rename(exe, &old) {
        let _ = fs::remove_file(&staged);
        return Err(err);
    }
    if let Err(err) = fs::rename(&staged, exe) {
        let _ = fs::rename(&old, exe);
        let _ = fs::remove_file(&staged);
        return Err(err);
    }
    let _ = fs::remove_file(&old);
    Ok(())
}

pub fn cleanup(exe: &Path) {
    let (Some(dir), Ok(name)) = (exe.parent(), file_name(exe)) else { return };
    let old_prefix = format!("{name}{OLD_MARKER}");
    let staging_prefix = format!(".{name}{STAGING_MARKER}");
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        if entry_name.starts_with(&old_prefix) || entry_name.starts_with(&staging_prefix) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn apply(
    source: &Source,
    manifest: &Manifest,
    exe: &Path,
    current: Option<&str>,
    version: &str,
    target: &str,
) -> io::Result<Outcome> {
    match plan(manifest, current, version, target) {
        Plan::UpToDate => Ok(Outcome::UpToDate),
        Plan::NoAsset => Ok(Outcome::NoAsset),
        Plan::Install { to, asset } => {
            let bytes = source.download(&asset)?;
            install(exe, &bytes)?;
            Ok(Outcome::Updated { to })
        }
    }
}

fn manifest_with_notice(source: Source) -> Option<(Source, Manifest)> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("orkai-update-check".into())
        .spawn(move || {
            let result = source.manifest();
            let _ = tx.send((source, result));
        })
        .ok()?;
    let received = match rx.recv_timeout(SLOW_CHECK_NOTICE) {
        Ok(received) => received,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let notice = "orkai: checking for updates…";
            eprint!("{notice}");
            let received = rx.recv().ok();
            eprint!("\r{}\r", " ".repeat(notice.chars().count()));
            received?
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => return None,
    };
    match received {
        (source, Ok(manifest)) => Some((source, manifest)),
        (_, Err(_)) => None,
    }
}

fn relaunch(exe: &Path, args: &[OsString], commit: &str) -> ExitCode {
    let mut command = Command::new(exe);
    command.args(args).env(UPDATED_ENV, commit);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = command.exec();
        eprintln!("orkai: could not start the updated version: {err}");
        ExitCode::FAILURE
    }
    #[cfg(not(unix))]
    match command.status() {
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).map_or(ExitCode::FAILURE, ExitCode::from),
        Err(err) => {
            eprintln!("orkai: could not start the updated version: {err}");
            ExitCode::FAILURE
        }
    }
}

pub fn on_launch(args: &[OsString]) -> Option<ExitCode> {
    if std::env::var_os(DISABLE_ENV).is_some() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    cleanup(&exe);
    if std::env::var_os(UPDATED_ENV).is_some() {
        return None;
    }
    let (current, target) = (BUILD_COMMIT?, current_target()?);
    let (source, manifest) = manifest_with_notice(Source::from_env())?;
    let version = env!("CARGO_PKG_VERSION");
    let Plan::Install { to, .. } = plan(&manifest, Some(current), version, target) else { return None };
    eprint!("orkai: updating {} → {}… ", short_commit(current), short_commit(&to));
    match apply(&source, &manifest, &exe, Some(current), version, target) {
        Ok(Outcome::Updated { to }) => {
            eprintln!("done");
            Some(relaunch(&exe, args, &to))
        }
        Ok(_) => {
            eprintln!("skipped");
            None
        }
        Err(err) => {
            eprintln!("failed: {err}");
            eprintln!("orkai: opening the current version");
            None
        }
    }
}

pub fn update_now() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    cleanup(&exe);
    let target = current_target()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "no prebuilt orkai exists for this platform"))?;
    let source = Source::from_env();
    let manifest = source.manifest()?;
    match apply(&source, &manifest, &exe, BUILD_COMMIT, env!("CARGO_PKG_VERSION"), target)? {
        Outcome::UpToDate => println!("orkai {} is up to date", VERSION.as_str()),
        Outcome::Updated { to } => println!("orkai updated to {} ({})", manifest.version, short_commit(&to)),
        Outcome::NoAsset => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the latest build has no binary for {target}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT_A: &str = "4645e00aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const COMMIT_B: &str = "9238b7abbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn manifest(target: &str) -> Manifest {
        let mut assets = BTreeMap::new();
        assets.insert(
            target.to_string(),
            Asset { name: format!("orkai-{target}"), sha256: sha256_hex(b"new binary"), size: 10 },
        );
        Manifest { commit: COMMIT_B.into(), version: "0.2.0".into(), built_at: String::new(), assets }
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orkai-update-{tag}-{}-{}", std::process::id(), crate::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn plan_installs_only_a_newer_version_for_the_target() {
        let win = "x86_64-pc-windows-msvc";
        let m = manifest(win);
        assert!(matches!(plan(&m, Some(COMMIT_A), "0.1.0", win), Plan::Install { .. }));
        assert!(matches!(plan(&m, Some(COMMIT_A), "0.1.9", win), Plan::Install { .. }));
        assert_eq!(plan(&m, Some(COMMIT_B), "0.2.0", win), Plan::UpToDate);
        assert_eq!(plan(&m, Some(COMMIT_A), "0.2.0", win), Plan::UpToDate, "a newer push of the same version stays");
        assert_eq!(plan(&m, Some(COMMIT_A), "0.10.0", win), Plan::UpToDate, "an older version never replaces it");
        assert!(matches!(plan(&m, None, "0.2.0", win), Plan::Install { .. }), "a local build becomes the release");
        assert_eq!(plan(&m, Some(COMMIT_A), "0.1.0", "riscv64-unknown-linux-gnu"), Plan::NoAsset);
        let mut unknown = manifest(win);
        unknown.version = "next".into();
        assert_eq!(plan(&unknown, Some(COMMIT_A), "0.1.0", win), Plan::UpToDate);
        assert_eq!(plan(&m, Some(COMMIT_A), "0.2.0-rc.1", win), Plan::UpToDate);
    }

    #[test]
    fn updates_follow_the_newest_version_and_not_every_push() {
        assert!(DEFAULT_BASE_URL.ends_with("/releases/latest/download"), "{DEFAULT_BASE_URL}");
    }

    #[test]
    fn manifest_parsing_rejects_unsafe_values() {
        let good = serde_json::to_vec(&manifest("x86_64-apple-darwin")).unwrap();
        assert!(Manifest::parse(&good).is_ok());
        let mut traversal = manifest("x86_64-apple-darwin");
        traversal.assets.get_mut("x86_64-apple-darwin").unwrap().name = "../../evil".into();
        assert!(Manifest::parse(&serde_json::to_vec(&traversal).unwrap()).is_err());
        let mut bad_hash = manifest("x86_64-apple-darwin");
        bad_hash.assets.get_mut("x86_64-apple-darwin").unwrap().sha256 = "zz".into();
        assert!(Manifest::parse(&serde_json::to_vec(&bad_hash).unwrap()).is_err());
        let mut bad_commit = manifest("x86_64-apple-darwin");
        bad_commit.commit = "main; rm -rf".into();
        assert!(Manifest::parse(&serde_json::to_vec(&bad_commit).unwrap()).is_err());
        assert!(Manifest::parse(b"not json").is_err());
    }

    #[test]
    fn install_replaces_the_executable_and_cleans_up() {
        let dir = temp_dir("install");
        let exe = dir.join(if cfg!(windows) { "orkai.exe" } else { "orkai" });
        fs::write(&exe, b"old binary").unwrap();
        install(&exe, b"new binary").unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new binary");
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers.len(), 1, "{leftovers:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o755);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cleanup_removes_previous_binaries_and_staging_files() {
        let dir = temp_dir("cleanup");
        let exe = dir.join("orkai.exe");
        fs::write(&exe, b"current").unwrap();
        fs::write(dir.join("orkai.exe.old-123"), b"old").unwrap();
        fs::write(dir.join(".orkai.exe.update-456"), b"partial").unwrap();
        fs::write(dir.join("other.exe"), b"keep").unwrap();
        cleanup(&exe);
        let mut names: Vec<String> =
            fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, vec!["orkai.exe".to_string(), "other.exe".to_string()]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn short_commit_and_version_label() {
        assert_eq!(short_commit(COMMIT_A), "4645e00");
        assert_eq!(short_commit("abc"), "abc");
        assert!(VERSION.starts_with(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn current_target_is_known_on_ci_platforms() {
        if cfg!(target_arch = "x86_64") || cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert!(current_target().is_some());
        }
    }
}
