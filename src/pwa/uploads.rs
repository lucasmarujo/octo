use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::paths::{Home, create_private_dir};
use crate::util::{random_bytes, to_hex};

pub const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_PART_BYTES: usize = 192 * 1024;
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_OPEN_UPLOADS: usize = 4;
pub const MAX_STORED_BYTES: u64 = 512 * 1024 * 1024;
pub const UNFINISHED_FOR: Duration = Duration::from_secs(10 * 60);
pub const STORED_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MARKER: &str = ".uploading";
const FALLBACK_NAME: &str = "file";
const SEPARATORS: [char; 2] = ['/', '\\'];
const FORBIDDEN: [char; 7] = ['<', '>', ':', '"', '|', '?', '*'];
const REPLACEMENT: char = '_';
const TRAILING: [char; 2] = ['.', ' '];
const DEVICES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
const NUMBERED_DEVICES: [&str; 2] = ["COM", "LPT"];
const FOLDER_BYTES: usize = 8;
const TOO_MANY: &str = "only 4 files can be sent at a time";
const NOT_STARTED: &str = "this file was not being received";
const OUT_OF_ORDER: &str = "the parts of this file arrived out of order";
const PART_TOO_LARGE: &str = "a part of this file is larger than 192 KiB";
const UNREADABLE: &str = "a part of this file could not be read";
const TOO_LARGE: &str = "the file is larger than 25 MiB";
const FULL: &str = "attachments folder is full";

fn is_device(stem: &str) -> bool {
    let stem = stem.trim_end_matches(TRAILING).to_ascii_uppercase();
    let numbered =
        |family: &&str| stem.strip_prefix(*family).is_some_and(|number| matches!(number.as_bytes(), [b'1'..=b'9']));
    DEVICES.contains(&stem.as_str()) || NUMBERED_DEVICES.iter().any(numbered)
}

pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(SEPARATORS).next().unwrap_or_default();
    let safe = |c: char| if FORBIDDEN.contains(&c) { REPLACEMENT } else { c };
    let kept: String = base.chars().filter(|c| !c.is_control()).map(safe).collect();
    let kept = kept.trim().trim_end_matches(TRAILING);
    let stem = kept.split('.').next().unwrap_or_default();
    let guarded = if is_device(stem) { format!("{REPLACEMENT}{kept}") } else { kept.to_string() };
    let cut: String = guarded.chars().take(MAX_NAME_CHARS).collect();
    let cut = cut.trim_end_matches(TRAILING);
    if cut.is_empty() || cut.eq_ignore_ascii_case(MARKER) { FALLBACK_NAME.to_string() } else { cut.to_string() }
}

fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn stored_bytes(home: &Home) -> u64 {
    let Ok(folders) = fs::read_dir(home.attachments_dir()) else { return 0 };
    let files = folders.flatten().filter_map(|folder| fs::read_dir(folder.path()).ok()).flat_map(Iterator::flatten);
    files.filter_map(|file| file.metadata().ok()).map(|meta| meta.len()).sum()
}

pub struct Part<'a> {
    pub seq: u64,
    pub name: &'a str,
    pub part: u32,
    pub last: bool,
    pub data: &'a str,
}

struct Upload {
    seq: u64,
    folder: PathBuf,
    path: PathBuf,
    file: File,
    next: u32,
    bytes: u64,
}

pub struct Uploads {
    home: Home,
    open: Vec<Upload>,
    budget: u64,
}

impl Drop for Uploads {
    fn drop(&mut self) {
        for upload in self.open.drain(..) {
            let _ = fs::remove_dir_all(&upload.folder);
        }
    }
}

fn failed(err: io::Error) -> String {
    format!("the file could not be stored: {err}")
}

impl Uploads {
    pub fn new(home: &Home) -> Self {
        Self::with_budget(home, MAX_STORED_BYTES)
    }

    pub fn with_budget(home: &Home, budget: u64) -> Self {
        Self { home: home.clone(), open: Vec::new(), budget }
    }

    pub fn open(&self) -> usize {
        self.open.len()
    }

    pub fn take(&mut self, part: &Part) -> Option<Result<PathBuf, String>> {
        match self.store(part) {
            Ok(None) => None,
            Ok(Some(path)) => Some(Ok(path)),
            Err(problem) => {
                self.abandon(part.seq);
                Some(Err(problem))
            }
        }
    }

    fn abandon(&mut self, seq: u64) {
        if let Some(at) = self.open.iter().position(|upload| upload.seq == seq) {
            let upload = self.open.remove(at);
            drop(upload.file);
            let _ = fs::remove_dir_all(&upload.folder);
        }
    }

    fn begin(&mut self, part: &Part) -> Result<(), String> {
        if self.open.iter().any(|upload| upload.seq == part.seq) {
            return Err(OUT_OF_ORDER.to_string());
        }
        if self.open.len() >= MAX_OPEN_UPLOADS {
            return Err(TOO_MANY.to_string());
        }
        if stored_bytes(&self.home) >= self.budget {
            return Err(FULL.to_string());
        }
        let folder = self.home.attachments_dir().join(to_hex(&random_bytes::<FOLDER_BYTES>().map_err(failed)?));
        create_private_dir(&folder).map_err(failed)?;
        let path = folder.join(clean_name(part.name));
        let created = create_new(&folder.join(MARKER)).and_then(|_| create_new(&path));
        match created {
            Ok(file) => {
                self.open.push(Upload { seq: part.seq, folder, path, file, next: 0, bytes: 0 });
                Ok(())
            }
            Err(err) => {
                let _ = fs::remove_dir_all(&folder);
                Err(failed(err))
            }
        }
    }

    fn store(&mut self, part: &Part) -> Result<Option<PathBuf>, String> {
        if part.part == 0 {
            self.begin(part)?;
        }
        let at = self.open.iter().position(|upload| upload.seq == part.seq).ok_or(NOT_STARTED)?;
        let upload = &mut self.open[at];
        if part.part != upload.next {
            return Err(OUT_OF_ORDER.to_string());
        }
        if part.data.len() > MAX_PART_BYTES.div_ceil(3) * 4 {
            return Err(PART_TOO_LARGE.to_string());
        }
        let bytes = STANDARD.decode(part.data).map_err(|_| UNREADABLE.to_string())?;
        upload.bytes += bytes.len() as u64;
        if upload.bytes > MAX_FILE_BYTES {
            return Err(TOO_LARGE.to_string());
        }
        upload.file.write_all(&bytes).map_err(failed)?;
        upload.next += 1;
        if !part.last {
            return Ok(None);
        }
        upload.file.flush().map_err(failed)?;
        fs::remove_file(upload.folder.join(MARKER)).map_err(failed)?;
        Ok(Some(self.open.remove(at).path))
    }
}

fn touched(folder: &Path) -> Option<SystemTime> {
    let files = fs::read_dir(folder).ok()?.flatten().filter_map(|entry| entry.metadata().ok()?.modified().ok());
    files.max().or_else(|| fs::metadata(folder).ok()?.modified().ok())
}

pub fn sweep(home: &Home, now: SystemTime) {
    let Ok(folders) = fs::read_dir(home.attachments_dir()) else { return };
    for folder in folders.flatten().map(|entry| entry.path()) {
        let keep_for = if folder.join(MARKER).exists() { UNFINISHED_FOR } else { STORED_FOR };
        let idle = touched(&folder).and_then(|at| now.duration_since(at).ok()).unwrap_or_default();
        if idle > keep_for {
            let _ = fs::remove_dir_all(&folder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;

    fn temp_home(tag: &str) -> (PathBuf, Home) {
        let dir = std::env::temp_dir().join(format!("orkai-uploads-{tag}-{}-{}", std::process::id(), now_ms()));
        (dir.clone(), Home::at(dir))
    }

    fn encoded(bytes: &[u8]) -> String {
        STANDARD.encode(bytes)
    }

    fn part<'a>(seq: u64, name: &'a str, part: u32, last: bool, data: &'a str) -> Part<'a> {
        Part { seq, name, part, last, data }
    }

    fn folders(home: &Home) -> usize {
        fs::read_dir(home.attachments_dir()).map_or(0, |entries| entries.count())
    }

    #[test]
    fn a_name_from_the_phone_becomes_a_plain_file_name() {
        assert_eq!(clean_name("shot one.png"), "shot one.png");
        assert_eq!(clean_name(r"..\..\x"), "x", "no path climbs out of the folder");
        assert_eq!(clean_name("a/b"), "b");
        assert_eq!(clean_name("/etc/passwd"), "passwd");
        assert_eq!(clean_name(r"C:\Users\me\notes.txt"), "notes.txt");
        assert_eq!(clean_name("re\u{0}port\n\t.pdf\u{7f}"), "report.pdf", "control characters are dropped");
        assert_eq!(clean_name(""), "file");
        assert_eq!(clean_name("   "), "file");
        assert_eq!(clean_name("\u{1}\u{2}"), "file");
        assert_eq!(clean_name("dir/"), "file", "a folder is not a file name");
        assert_eq!(clean_name(".."), "file");
        assert_eq!(clean_name("a/.."), "file");
        assert_eq!(clean_name("."), "file");
        assert_eq!(clean_name(".env"), ".env");
        assert_eq!(clean_name("..."), "file");
        assert_eq!(
            clean_name(r#"a<b>c:d"e|f?g*.txt"#),
            "a_b_c_d_e_f_g_.txt",
            "what Windows forbids becomes an underscore"
        );
        assert_eq!(clean_name("notes.txt:hidden"), "notes.txt_hidden", "no alternate stream is addressed");
        assert_eq!(clean_name("report. . "), "report", "Windows drops trailing dots and spaces, so they go first");
        for device in ["CON", "con", "PRN.txt", "aux.tar.gz", "NUL", "COM1", "com9.log", "LPT1", "lpt9.txt", "CON .txt"]
        {
            assert_eq!(clean_name(device), format!("_{device}").trim_end_matches(['.', ' ']), "{device} is a device");
        }
        for plain in ["CONSOLE.txt", "COM0", "COM10.txt", "LPT", "nul2", "icon"] {
            assert_eq!(clean_name(plain), plain, "{plain} only looks like one");
        }
        assert_eq!(clean_name(".uploading"), "file", "the name that marks an upload in progress is never a file");
        assert_eq!(clean_name(".UPLOADING"), "file");
        let reserved = clean_name(&format!("con.{}", "x".repeat(MAX_NAME_CHARS * 2)));
        assert_eq!(reserved.chars().count(), MAX_NAME_CHARS);
        assert!(reserved.starts_with("_con."));
        let long = clean_name(&format!("{}.png", "ç".repeat(MAX_NAME_CHARS * 2)));
        assert_eq!(long.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn a_file_arrives_in_parts_and_is_answered_once_with_its_path() {
        let (dir, home) = temp_home("parts");
        let mut uploads = Uploads::new(&home);
        let (first, second) = (encoded(b"hello "), encoded(b"world"));
        assert_eq!(uploads.take(&part(7, r"..\..\shot one.png", 0, false, &first)), None, "more parts are expected");
        assert_eq!(uploads.open(), 1);
        assert_eq!(uploads.take(&part(7, "ignored", 1, false, &second)), None);
        let stored = uploads.take(&part(7, "", 2, true, "")).unwrap().unwrap();
        assert_eq!(fs::read(&stored).unwrap(), b"hello world");
        assert_eq!(stored.file_name().unwrap(), "shot one.png");
        let folder = stored.parent().unwrap();
        assert_eq!(folder.parent().unwrap(), home.attachments_dir(), "one folder of its own under attachments");
        assert_eq!(folder.file_name().unwrap().len(), 16);
        assert!(!folder.join(MARKER).exists(), "a finished file is no longer marked as arriving");
        assert!(stored.is_absolute() || home.root().is_relative());
        assert_eq!(uploads.open(), 0);

        let whole = uploads.take(&part(8, "one.txt", 0, true, &encoded(b"x"))).unwrap().unwrap();
        assert_ne!(whole.parent(), stored.parent(), "two files with any names never collide");
        let empty = uploads.take(&part(9, "empty", 0, true, "")).unwrap().unwrap();
        assert_eq!(fs::read(&empty).unwrap(), b"");
        assert_eq!(
            uploads.take(&part(7, "again.txt", 0, true, &encoded(b"y"))).map(|stored| stored.is_ok()),
            Some(true)
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_part_out_of_order_or_unreadable_fails_the_file_and_leaves_nothing_behind() {
        let (dir, home) = temp_home("order");
        let mut uploads = Uploads::new(&home);
        let data = encoded(b"abc");
        assert_eq!(uploads.take(&part(1, "a.txt", 1, false, &data)), Some(Err(NOT_STARTED.to_string())));
        assert_eq!(folders(&home), 0);

        assert_eq!(uploads.take(&part(2, "a.txt", 0, false, &data)), None);
        assert_eq!(uploads.take(&part(2, "a.txt", 2, false, &data)), Some(Err(OUT_OF_ORDER.to_string())));
        assert_eq!((uploads.open(), folders(&home)), (0, 0), "what was received is deleted");
        assert_eq!(
            uploads.take(&part(2, "a.txt", 1, true, &data)),
            Some(Err(NOT_STARTED.to_string())),
            "and it is over"
        );

        assert_eq!(uploads.take(&part(3, "a.txt", 0, false, &data)), None);
        assert_eq!(uploads.take(&part(3, "a.txt", 0, false, &data)), Some(Err(OUT_OF_ORDER.to_string())));
        assert_eq!(uploads.take(&part(4, "a.txt", 0, true, "not base64!")), Some(Err(UNREADABLE.to_string())));
        assert_eq!((uploads.open(), folders(&home)), (0, 0));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_part_or_a_file_over_the_limit_is_refused() {
        let (dir, home) = temp_home("limits");
        let mut uploads = Uploads::new(&home);
        let full = encoded(&vec![b'x'; MAX_PART_BYTES]);
        let over = encoded(&vec![b'x'; MAX_PART_BYTES + 1]);
        assert_eq!(uploads.take(&part(1, "big.bin", 0, false, &over)), Some(Err(PART_TOO_LARGE.to_string())));
        assert_eq!(folders(&home), 0);
        let parts = u32::try_from(MAX_FILE_BYTES / MAX_PART_BYTES as u64).unwrap();
        for index in 0..parts {
            assert_eq!(uploads.take(&part(2, "big.bin", index, false, &full)), None, "part {index}");
        }
        let fits = MAX_FILE_BYTES - u64::from(parts) * MAX_PART_BYTES as u64;
        let rest = encoded(&vec![b'y'; usize::try_from(fits).unwrap()]);
        let stored = uploads.take(&part(2, "big.bin", parts, true, &rest)).unwrap().unwrap();
        assert_eq!(fs::metadata(&stored).unwrap().len(), MAX_FILE_BYTES, "25 MiB exactly is fine");

        for index in 0..parts {
            assert_eq!(uploads.take(&part(3, "huge.bin", index, false, &full)), None);
        }
        let one_more = encoded(&vec![b'y'; usize::try_from(fits).unwrap() + 1]);
        assert_eq!(uploads.take(&part(3, "huge.bin", parts, true, &one_more)), Some(Err(TOO_LARGE.to_string())));
        assert_eq!((uploads.open(), folders(&home)), (0, 1), "only the file that fit stays");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_phone_sends_four_files_at_a_time_and_a_fifth_waits_for_one_to_end() {
        let (dir, home) = temp_home("four");
        let mut uploads = Uploads::new(&home);
        let data = encoded(b"abc");
        for seq in 1..=4 {
            assert_eq!(uploads.take(&part(seq, "a.txt", 0, false, &data)), None);
        }
        assert_eq!(uploads.take(&part(5, "a.txt", 0, false, &data)), Some(Err(TOO_MANY.to_string())));
        assert_eq!((uploads.open(), folders(&home)), (4, 4), "the four in flight are not disturbed");
        assert!(uploads.take(&part(2, "a.txt", 1, true, &data)).unwrap().is_ok());
        assert_eq!(uploads.take(&part(5, "a.txt", 0, false, &data)), None, "room was made");
        drop(uploads);
        assert_eq!(folders(&home), 1, "a phone that leaves takes its unfinished files with it");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_full_attachments_folder_takes_no_new_file_until_something_is_deleted() {
        let (dir, home) = temp_home("budget");
        let data = encoded(&[b'x'; 600]);
        let mut uploads = Uploads::with_budget(&home, 1_000);
        let first = uploads.take(&part(1, "a.bin", 0, true, &data)).unwrap().unwrap();
        assert!(uploads.take(&part(2, "b.bin", 0, true, &data)).unwrap().is_ok(), "600 bytes are under the budget");
        assert_eq!(uploads.take(&part(3, "c.bin", 0, true, &data)), Some(Err(FULL.to_string())));
        assert_eq!(folders(&home), 2, "a refused file leaves nothing behind");
        fs::remove_dir_all(first.parent().unwrap()).unwrap();
        assert!(uploads.take(&part(4, "d.bin", 0, true, &data)).unwrap().is_ok(), "room was made");
        assert_eq!(stored_bytes(&home), 1_200);
        assert_eq!(stored_bytes(&Home::at(dir.join("nowhere"))), 0);
        assert!(Uploads::new(&home).budget == MAX_STORED_BYTES && MAX_STORED_BYTES == 512 * 1024 * 1024);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_is_never_written_over_another_one() {
        let (dir, _home) = temp_home("create-new");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("once.txt");
        create_new(&path).unwrap().write_all(b"first").unwrap();
        assert_eq!(create_new(&path).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"first");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_files_and_uploads_left_unfinished_are_deleted_when_the_bridge_starts() {
        let (dir, home) = temp_home("sweep");
        sweep(&home, SystemTime::now());
        let mut uploads = Uploads::new(&home);
        let data = encoded(b"abc");
        let stored = uploads.take(&part(1, "kept.txt", 0, true, &data)).unwrap().unwrap();
        let old = uploads.take(&part(2, "old.txt", 0, true, &data)).unwrap().unwrap();
        let unfinished: Vec<PathBuf> = [3, 4]
            .into_iter()
            .map(|seq| {
                let path = uploads.take(&part(seq, "half.bin", 0, true, &data)).unwrap().unwrap();
                let folder = path.parent().unwrap().to_path_buf();
                fs::write(folder.join(MARKER), b"").unwrap();
                folder
            })
            .collect();
        let now = SystemTime::now();
        let age = |path: &Path, by: Duration| {
            for entry in fs::read_dir(path).unwrap().flatten() {
                File::options().write(true).open(entry.path()).unwrap().set_modified(now - by).unwrap();
            }
        };
        age(old.parent().unwrap(), STORED_FOR + Duration::from_secs(60));
        age(stored.parent().unwrap(), STORED_FOR - Duration::from_secs(60));
        age(&unfinished[0], UNFINISHED_FOR - Duration::from_secs(60));
        age(&unfinished[1], UNFINISHED_FOR + Duration::from_secs(60));
        sweep(&home, now);
        assert!(stored.exists(), "a file sent this week stays");
        assert!(!old.parent().unwrap().exists(), "a file older than a week goes with its folder");
        assert!(unfinished[0].exists(), "an upload still arriving stays");
        assert!(!unfinished[1].exists(), "an upload abandoned ten minutes ago goes");
        fs::remove_dir_all(dir).unwrap();
    }
}
