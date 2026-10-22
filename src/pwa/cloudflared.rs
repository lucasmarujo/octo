use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::fsutil;
use crate::launch::{self, without_console};
use crate::paths::Home;
use crate::update::sha256_hex;

pub const RELEASE_URL: &str = "https://api.github.com/repos/cloudflare/cloudflared/releases/latest";
const RELEASE_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_RELEASE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 160 * 1024 * 1024;
const PROGRAM: &str = "cloudflared";
const DIGEST_PREFIX: &str = "sha256:";
const DOWNLOAD_HOST: &str = "https://github.com/cloudflare/cloudflared/releases/download/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

pub fn asset_name() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("cloudflared-windows-amd64.exe")
    } else if cfg!(all(windows, target_arch = "x86")) {
        Some("cloudflared-windows-386.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("cloudflared-linux-amd64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("cloudflared-linux-arm64")
    } else if cfg!(all(target_os = "linux", target_arch = "arm")) {
        Some("cloudflared-linux-arm")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("cloudflared-darwin-arm64.tgz")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("cloudflared-darwin-amd64.tgz")
    } else {
        None
    }
}

pub fn installed_path(home: &Home) -> PathBuf {
    home.bin_dir().join(format!("{PROGRAM}{}", std::env::consts::EXE_SUFFIX))
}

pub fn locate(home: &Home) -> Option<PathBuf> {
    launch::resolve_program(PROGRAM).ok().or_else(|| Some(installed_path(home)).filter(|path| path.is_file()))
}

pub fn release_asset(release: &[u8], name: &str) -> io::Result<Release> {
    let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidData, format!("cloudflared release: {message}"));
    let value: serde_json::Value = serde_json::from_slice(release).map_err(|_| invalid("not json"))?;
    let asset = value
        .get("assets")
        .and_then(|assets| assets.as_array())
        .and_then(|assets| assets.iter().find(|asset| asset.get("name").and_then(|n| n.as_str()) == Some(name)))
        .ok_or_else(|| invalid(&format!("no build named {name}")))?;
    let url = asset.get("browser_download_url").and_then(|u| u.as_str()).unwrap_or_default();
    if !url.starts_with(DOWNLOAD_HOST) {
        return Err(invalid("the download is not on github.com/cloudflare"));
    }
    let sha256 = asset
        .get("digest")
        .and_then(|d| d.as_str())
        .and_then(|d| d.strip_prefix(DIGEST_PREFIX))
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| invalid("the build has no sha256 digest"))?;
    let size = asset.get("size").and_then(|s| s.as_u64()).filter(|size| *size <= MAX_ASSET_BYTES).unwrap_or(0);
    if size == 0 {
        return Err(invalid("the build has no size"));
    }
    Ok(Release { url: url.to_string(), sha256: sha256.to_ascii_lowercase(), size })
}

pub fn verify(bytes: &[u8], release: &Release) -> io::Result<()> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != release.size {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the cloudflared download is incomplete"));
    }
    if !sha256_hex(bytes).eq_ignore_ascii_case(&release.sha256) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the cloudflared download failed its sha256 check"));
    }
    Ok(())
}

fn fetch(url: &str, timeout: Duration, limit: u64) -> io::Result<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(true)
        .user_agent(format!("orkai/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| io::Error::other(format!("{url}: {e}")))?;
    response.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| io::Error::other(format!("{url}: {e}")))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn unpack(name: &str, bytes: Vec<u8>, staging: &Path) -> io::Result<Vec<u8>> {
    if !name.ends_with(".tgz") {
        return Ok(bytes);
    }
    let archive = staging.join(name);
    fs::write(&archive, &bytes)?;
    let status = without_console(&mut Command::new("tar")).arg("-xzf").arg(&archive).arg("-C").arg(staging).status()?;
    if !status.success() {
        return Err(io::Error::other("could not unpack cloudflared"));
    }
    fs::read(staging.join(PROGRAM))
}

pub fn install(home: &Home) -> io::Result<PathBuf> {
    let name = asset_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Unsupported, "no cloudflared build exists for this platform; install it yourself")
    })?;
    let release = release_asset(&fetch(RELEASE_URL, RELEASE_TIMEOUT, MAX_RELEASE_BYTES)?, name)?;
    let bytes = fetch(&release.url, DOWNLOAD_TIMEOUT, MAX_ASSET_BYTES)?;
    verify(&bytes, &release)?;
    let staging = home.bin_dir().join(format!(".cloudflared-{}", std::process::id()));
    fs::create_dir_all(&staging)?;
    let unpacked = unpack(name, bytes, &staging);
    let _ = fsutil::remove_dir_all_retry(&staging);
    let target = installed_path(home);
    fsutil::write_atomic(&target, &unpacked?)?;
    make_executable(&target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(asset: &str) -> Vec<u8> {
        format!(r#"{{"tag_name":"2026.10.3","assets":[{asset}]}}"#).into_bytes()
    }

    const GOOD: &str = r#"{"name":"cloudflared-linux-amd64","size":5,"digest":"sha256:2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824","browser_download_url":"https://github.com/cloudflare/cloudflared/releases/download/2026.10.3/cloudflared-linux-amd64"}"#;

    #[test]
    fn this_platform_has_a_cloudflared_build() {
        assert!(asset_name().is_some());
    }

    #[test]
    fn the_release_names_the_url_size_and_digest_of_the_build() {
        let found = release_asset(&release(GOOD), "cloudflared-linux-amd64").unwrap();
        assert_eq!(found.size, 5);
        assert_eq!(found.sha256, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert!(found.url.ends_with("/cloudflared-linux-amd64"));
        verify(b"hello", &found).unwrap();
    }

    #[test]
    fn a_build_that_does_not_match_its_digest_is_refused() {
        let found = release_asset(&release(GOOD), "cloudflared-linux-amd64").unwrap();
        assert_eq!(verify(b"hellO", &found).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(verify(b"hello!", &found).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_release_without_a_digest_or_off_github_is_refused() {
        let no_digest =
            GOOD.replace(r#""digest":"sha256:2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824","#, "");
        assert!(release_asset(&release(&no_digest), "cloudflared-linux-amd64").is_err());
        let elsewhere = GOOD.replace("https://github.com/cloudflare/", "https://example.com/cloudflare/");
        assert!(release_asset(&release(&elsewhere), "cloudflared-linux-amd64").is_err());
        assert!(release_asset(&release(GOOD), "cloudflared-windows-amd64.exe").is_err());
        assert!(release_asset(b"not json", "cloudflared-linux-amd64").is_err());
    }

    #[test]
    fn the_installed_copy_lives_in_the_orkai_bin_folder() {
        let home = Home::at("/tmp/orkai-home");
        let path = installed_path(&home);
        assert!(path.starts_with(home.bin_dir()));
        assert!(path.file_name().unwrap().to_string_lossy().starts_with(PROGRAM));
    }
}
