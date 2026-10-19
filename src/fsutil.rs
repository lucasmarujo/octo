use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
const REPLACE_ATTEMPTS: u32 = 20;
const REPLACE_BACKOFF: Duration = Duration::from_millis(15);

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let data = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    write_atomic(path, &data)
}

pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .into_owned();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!(".{file_name}.{}.{counter}.tmp", std::process::id()));
    {
        let mut file = create_private_file(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    replace_file(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(err) if attempt + 1 < REPLACE_ATTEMPTS && is_transient(&err) => {
                attempt += 1;
                std::thread::sleep(REPLACE_BACKOFF);
            }
            Err(err) => return Err(err),
        }
    }
}

fn is_transient(err: &io::Error) -> bool {
    matches!(err.kind(), io::ErrorKind::PermissionDenied) || err.raw_os_error() == Some(32)
}

pub fn write_if_changed(path: &Path, data: &[u8]) -> io::Result<bool> {
    if fs::read(path).is_ok_and(|current| current == data) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_atomic(path, data)?;
    Ok(true)
}

pub fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let data = fs::read(path)?;
    serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn read_json_opt<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match read_json(path) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn remove_dir_all_retry(path: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) if attempt + 1 < REPLACE_ATTEMPTS && is_transient(&err) => {
                attempt += 1;
                std::thread::sleep(REPLACE_BACKOFF * 2);
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Sample {
        name: String,
        value: u32,
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orkai-fsutil-{tag}-{}-{}", std::process::id(), crate::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_round_trips_and_replaces() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("sample.json");
        write_json_atomic(&path, &Sample { name: "a".into(), value: 1 }).unwrap();
        write_json_atomic(&path, &Sample { name: "b".into(), value: 2 }).unwrap();
        let loaded: Sample = read_json(&path).unwrap();
        assert_eq!(loaded, Sample { name: "b".into(), value: 2 });
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn write_if_changed_skips_identical_content_and_creates_folders() {
        let dir = temp_dir("changed");
        let path = dir.join("nested").join("file.bin");
        assert!(write_if_changed(&path, b"one").unwrap());
        assert!(!write_if_changed(&path, b"one").unwrap());
        assert!(write_if_changed(&path, b"two").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"two");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn read_json_opt_returns_none_for_missing_file() {
        let dir = temp_dir("missing");
        let value: Option<Sample> = read_json_opt(&dir.join("nope.json")).unwrap();
        assert!(value.is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn read_json_reports_invalid_data() {
        let dir = temp_dir("invalid");
        let path = dir.join("bad.json");
        fs::write(&path, b"{not json").unwrap();
        let err = read_json::<Sample>(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remove_helpers_tolerate_missing_paths() {
        let dir = temp_dir("remove");
        remove_file_if_exists(&dir.join("absent")).unwrap();
        remove_dir_all_retry(&dir.join("absent-dir")).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
