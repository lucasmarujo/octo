use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::LazyLock;
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hooks::parse_version;
use crate::util::to_hex;

pub const DEFAULT_BASE_URL: &str = "https://github.com/lucasmarujo/octo/releases/latest/download";
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
const BUNDLE_EXTENSION: &str = "app";
const BUNDLE_CONTENTS: &str = "Contents";
const BUNDLE_EXECUTABLES: &str = "MacOS";
const BUNDLE_ARCHIVE: &str = "download.zip";
const DITTO: &str = "/usr/bin/ditto";

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

pub fn app_bundle(exe: &Path) -> Option<&Path> {
    let executables = exe.parent()?;
    let contents = executables.parent()?;
    let bundle = contents.parent()?;
    let inside = executables.file_name()? == BUNDLE_EXECUTABLES
        && contents.file_name()? == BUNDLE_CONTENTS
        && bundle.extension()? == BUNDLE_EXTENSION;
    inside.then_some(bundle)
}

fn unpack_app(archive: &[u8], staging: &Path) -> io::Result<PathBuf> {
    let _ = fs::remove_dir_all(staging);
    fs::create_dir(staging)?;
    let zip = staging.join(BUNDLE_ARCHIVE);
    fs::write(&zip, archive)?;
    let status = Command::new(DITTO).arg("-x").arg("-k").arg(&zip).arg(staging).status()?;
    fs::remove_file(&zip)?;
    if !status.success() {
        return Err(invalid("the downloaded app could not be unpacked"));
    }
    fs::read_dir(staging)?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == BUNDLE_EXTENSION))
        .ok_or_else(|| invalid("the download has no app"))
}

pub fn install_app(exe: &Path, archive: &[u8]) -> io::Result<()> {
    let bundle = app_bundle(exe).ok_or_else(|| invalid("the executable is not inside an app bundle"))?;
    let executable = exe.strip_prefix(bundle).map_err(|_| invalid("the executable is outside its app bundle"))?;
    let dir = bundle.parent().ok_or_else(|| invalid("the app bundle has no parent"))?;
    let name = file_name(bundle)?;
    let pid = std::process::id();
    let staging = dir.join(format!(".{name}{STAGING_MARKER}{pid}"));
    let old = dir.join(format!("{name}{OLD_MARKER}{pid}"));
    let result = unpack_app(archive, &staging).and_then(|unpacked| {
        if !unpacked.join(executable).is_file() {
            return Err(invalid("the downloaded app has no executable"));
        }
        fs::rename(bundle, &old)?;
        if let Err(err) = fs::rename(&unpacked, bundle) {
            let _ = fs::rename(&old, bundle);
            return Err(err);
        }
        let _ = fs::remove_dir_all(&old);
        Ok(())
    });
    let _ = fs::remove_dir_all(&staging);
    result
}

fn remove_leftovers(dir: &Path, name: &str) {
    let old_prefix = format!("{name}{OLD_MARKER}");
    let staging_prefix = format!(".{name}{STAGING_MARKER}");
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        if entry_name.starts_with(&old_prefix) || entry_name.starts_with(&staging_prefix) {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                let _ = fs::remove_dir_all(entry.path());
            } else {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

pub fn cleanup(exe: &Path) {
    let (Some(dir), Ok(name)) = (exe.parent(), file_name(exe)) else { return };
    remove_leftovers(dir, &name);
}

pub fn cleanup_app(exe: &Path) {
    let Some(bundle) = app_bundle(exe) else { return };
    let (Some(dir), Ok(name)) = (bundle.parent(), file_name(bundle)) else { return };
    remove_leftovers(dir, &name);
}

pub fn apply(
    source: &Source,
    manifest: &Manifest,
    exe: &Path,
    current: Option<&str>,
    version: &str,
    target: &str,
) -> io::Result<Outcome> {
    apply_with(source, manifest, current, version, target, |bytes| install(exe, bytes))
}

pub fn apply_with(
    source: &Source,
    manifest: &Manifest,
    current: Option<&str>,
    version: &str,
    target: &str,
    installer: impl FnOnce(&[u8]) -> io::Result<()>,
) -> io::Result<Outcome> {
    match plan(manifest, current, version, target) {
        Plan::UpToDate => Ok(Outcome::UpToDate),
        Plan::NoAsset => Ok(Outcome::NoAsset),
        Plan::Install { to, asset } => {
            let bytes = source.download(&asset)?;
            installer(&bytes)?;
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

    fn fake_app(bundle: &Path, executable: &[u8]) -> std::path::PathBuf {
        let exe = bundle.join("Contents").join("MacOS").join("octo");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, executable).unwrap();
        fs::write(bundle.join("Contents").join("Info.plist"), executable).unwrap();
        exe
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn app_bundle_is_the_app_an_executable_runs_from() {
        let bundle = Path::new("/Applications/Octo.app");
        assert_eq!(app_bundle(&bundle.join("Contents/MacOS/octo")), Some(bundle));
        assert_eq!(app_bundle(Path::new("/usr/local/bin/orkai")), None);
        assert_eq!(app_bundle(Path::new("/Applications/Octo/Contents/MacOS/octo")), None, "not an app");
        assert_eq!(app_bundle(Path::new("/Applications/Octo.app/Contents/Resources/octo")), None);
        assert_eq!(app_bundle(Path::new("octo")), None);
    }

    #[test]
    fn install_app_refuses_an_executable_outside_an_app_and_leaves_it_alone() {
        let dir = temp_dir("not-an-app");
        let exe = dir.join("octo");
        fs::write(&exe, b"current").unwrap();
        assert!(install_app(&exe, b"archive").is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"current");
        assert_eq!(names_in(&dir), vec!["octo".to_string()]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cleanup_app_removes_previous_bundles_and_staging_folders_beside_the_app() {
        let dir = temp_dir("cleanup-app");
        let exe = fake_app(&dir.join("Octo.app"), b"current");
        fake_app(&dir.join("Octo.app.old-123"), b"old");
        fake_app(&dir.join(".Octo.app.update-456"), b"partial");
        fs::write(dir.join(".Octo.app.update-789"), b"stray file").unwrap();
        fake_app(&dir.join("Other.app"), b"keep");
        cleanup_app(&exe);
        assert_eq!(names_in(&dir), vec!["Octo.app".to_string(), "Other.app".to_string()]);
        assert_eq!(fs::read(&exe).unwrap(), b"current");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    fn zip_app(bundle: &Path, archive: &Path) -> Vec<u8> {
        let status = Command::new(DITTO).arg("-c").arg("-k").arg("--keepParent").arg(bundle).arg(archive).status();
        assert!(status.unwrap().success());
        fs::read(archive).unwrap()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_app_replaces_the_whole_bundle_and_keeps_its_name() {
        let release = temp_dir("app-release");
        fake_app(&release.join("Octo.app"), b"new binary");
        fs::create_dir_all(release.join("Octo.app/Contents/Resources")).unwrap();
        fs::write(release.join("Octo.app/Contents/Resources/icon.icns"), b"new icon").unwrap();
        let archive = zip_app(&release.join("Octo.app"), &release.join("Octo-macos.zip"));
        let installed = temp_dir("app-installed");
        let exe = fake_app(&installed.join("My Octo.app"), b"old binary");
        fs::write(installed.join("My Octo.app/Contents/stale"), b"gone").unwrap();
        install_app(&exe, &archive).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new binary");
        assert_eq!(fs::read(installed.join("My Octo.app/Contents/Info.plist")).unwrap(), b"new binary");
        assert_eq!(fs::read(installed.join("My Octo.app/Contents/Resources/icon.icns")).unwrap(), b"new icon");
        assert!(!installed.join("My Octo.app/Contents/stale").exists(), "nothing of the old bundle is left inside");
        assert_eq!(names_in(&installed), vec!["My Octo.app".to_string()]);
        fs::remove_dir_all(release).unwrap();
        fs::remove_dir_all(installed).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_app_keeps_the_running_bundle_when_the_download_is_not_a_usable_app() {
        let release = temp_dir("app-broken");
        let renamed = release.join("Octo.app");
        fake_app(&renamed, b"new binary");
        fs::rename(renamed.join("Contents/MacOS/octo"), renamed.join("Contents/MacOS/other")).unwrap();
        let without_executable = zip_app(&renamed, &release.join("Octo-macos.zip"));
        let installed = temp_dir("app-kept");
        let exe = fake_app(&installed.join("Octo.app"), b"old binary");
        for archive in [b"not a zip".to_vec(), without_executable] {
            assert!(install_app(&exe, &archive).is_err());
            assert_eq!(fs::read(&exe).unwrap(), b"old binary");
            assert_eq!(names_in(&installed), vec!["Octo.app".to_string()]);
        }
        fs::remove_dir_all(release).unwrap();
        fs::remove_dir_all(installed).unwrap();
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
