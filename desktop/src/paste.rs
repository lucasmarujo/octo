use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use orkai::paths::create_private_dir;
use orkai::util::now_ms;
use tauri::State;
use tauri::ipc::{InvokeBody, Request};

use crate::link::Link;

pub const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const KEPT_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG: &[u8] = b"\xff\xd8\xff";
const GIF: &[u8] = b"GIF8";
const BMP: &[u8] = b"BM";
const RIFF: &[u8] = b"RIFF";
const WEBP: &[u8] = b"WEBP";
const WEBP_AT: usize = 8;

static PASTED: AtomicU64 = AtomicU64::new(0);

fn extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(PNG) {
        Some("png")
    } else if bytes.starts_with(JPEG) {
        Some("jpg")
    } else if bytes.starts_with(GIF) {
        Some("gif")
    } else if bytes.starts_with(RIFF) && bytes.get(WEBP_AT..WEBP_AT + WEBP.len()) == Some(WEBP) {
        Some("webp")
    } else if bytes.starts_with(BMP) {
        Some("bmp")
    } else {
        None
    }
}

fn prune(dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
        let old = modified.and_then(|at| now.duration_since(at).ok()).is_some_and(|age| age > KEPT_FOR);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn save(dir: &Path, bytes: &[u8], now: SystemTime) -> Result<PathBuf, String> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!("the image is larger than {} MB", MAX_IMAGE_BYTES / (1024 * 1024)));
    }
    let extension = extension(bytes).ok_or_else(|| "this is not an image that can be pasted".to_string())?;
    create_private_dir(dir).map_err(|err| err.to_string())?;
    prune(dir, now);
    let count = PASTED.fetch_add(1, Ordering::SeqCst) + 1;
    let path = dir.join(format!("image-{}-{}-{count}.{extension}", now_ms(), std::process::id()));
    std::fs::write(&path, bytes).map_err(|err| err.to_string())?;
    Ok(path)
}

#[tauri::command(async)]
pub fn paste_image(link: State<'_, Link>, request: Request<'_>) -> Result<String, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("this is not an image that can be pasted".to_string());
    };
    let dir = link.hub().store().home().pasted_dir();
    save(&dir, bytes, SystemTime::now()).map(|path| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        Scratch(std::env::temp_dir().join(format!("octo-paste-{name}-{}-{}", std::process::id(), now_ms())))
    }

    fn png() -> Vec<u8> {
        [PNG, b"pixels"].concat()
    }

    #[test]
    fn a_pasted_image_becomes_a_file_the_agent_can_read_named_by_what_it_is() {
        let scratch = scratch("save");
        let dir = scratch.0.join("pasted");
        let first = save(&dir, &png(), SystemTime::now()).unwrap();
        let second = save(&dir, &png(), SystemTime::now()).unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), png());
        assert!(first.is_absolute() && first.starts_with(&dir), "{first:?}");
        assert_eq!(first.extension().unwrap(), "png");
        assert_ne!(first, second, "two pastes never share a file");
        for (bytes, wanted) in [
            (&b"\xff\xd8\xff\xe0jfif"[..], "jpg"),
            (&b"GIF89a"[..], "gif"),
            (&b"RIFF\x10\0\0\0WEBPVP8 "[..], "webp"),
            (&b"BM\x36\0\0\0"[..], "bmp"),
        ] {
            assert_eq!(save(&dir, bytes, SystemTime::now()).unwrap().extension().unwrap(), wanted);
        }
    }

    #[test]
    fn what_is_not_an_image_or_is_too_large_is_refused_and_writes_nothing() {
        let scratch = scratch("refuse");
        let dir = scratch.0.join("pasted");
        for bytes in [&b""[..], &b"MZ\x90\0program"[..], &b"RIFF\x10\0\0\0WAVEfmt "[..], &b"#!/bin/sh\n"[..]] {
            let err = save(&dir, bytes, SystemTime::now()).unwrap_err();
            assert!(err.contains("not an image"), "{err}");
        }
        let mut huge = png();
        huge.resize(MAX_IMAGE_BYTES + 1, 0);
        let err = save(&dir, &huge, SystemTime::now()).unwrap_err();
        assert!(err.contains("larger than 32 MB"), "{err}");
        assert!(!dir.exists(), "nothing was written");
    }

    #[test]
    fn pasting_clears_the_images_of_past_weeks_and_keeps_the_recent_ones() {
        let scratch = scratch("prune");
        let dir = scratch.0.join("pasted");
        let now = SystemTime::now();
        let old = save(&dir, &png(), now).unwrap();
        let recent = save(&dir, &png(), now).unwrap();
        File::options().write(true).open(&old).unwrap().set_modified(now - KEPT_FOR - Duration::from_secs(60)).unwrap();
        File::options()
            .write(true)
            .open(&recent)
            .unwrap()
            .set_modified(now - KEPT_FOR + Duration::from_secs(60))
            .unwrap();
        let fresh = save(&dir, &png(), now).unwrap();
        assert!(!old.exists(), "the image of last week is gone");
        assert!(recent.exists() && fresh.exists());
    }
}
