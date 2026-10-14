use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::protocol::TermSize;

pub const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
pub const REPLAY_SCROLLBACK: usize = 5_000;
const TAG_OUTPUT: u8 = 1;
const TAG_RESIZE: u8 = 2;
const MAX_RECORD_LEN: usize = 4 * 1024 * 1024;

pub struct OutputLog {
    path: PathBuf,
    file: BufWriter<File>,
    written: u64,
    size: TermSize,
    max_bytes: u64,
}

impl OutputLog {
    pub fn open(path: PathBuf, size: TermSize) -> io::Result<Self> {
        Self::open_with_limit(path, size, MAX_LOG_BYTES)
    }

    pub fn open_with_limit(path: PathBuf, size: TermSize, max_bytes: u64) -> io::Result<Self> {
        let file = open_append(&path)?;
        let written = file.metadata()?.len();
        let mut log = Self { path, file: BufWriter::new(file), written, size, max_bytes };
        log.write_resize_record()?;
        Ok(log)
    }

    pub fn output(&mut self, bytes: &[u8]) -> io::Result<()> {
        for chunk in bytes.chunks(MAX_RECORD_LEN) {
            self.write_record(TAG_OUTPUT, chunk)?;
        }
        if self.written >= self.max_bytes {
            self.rotate()?;
        }
        Ok(())
    }

    pub fn resize(&mut self, size: TermSize) -> io::Result<()> {
        if size != self.size {
            self.size = size;
            self.write_resize_record()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }

    fn write_resize_record(&mut self) -> io::Result<()> {
        let mut payload = [0u8; 4];
        payload[..2].copy_from_slice(&self.size.rows.to_le_bytes());
        payload[2..].copy_from_slice(&self.size.cols.to_le_bytes());
        self.write_record(TAG_RESIZE, &payload)
    }

    fn write_record(&mut self, tag: u8, payload: &[u8]) -> io::Result<()> {
        let len = u32::try_from(payload.len()).map_err(|_| io::Error::other("record too large"))?;
        self.file.write_all(&[tag])?;
        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(payload)?;
        self.written += 5 + u64::from(len);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let rotated = rotated_path(&self.path);
        let _ = fs::remove_file(&rotated);
        fs::rename(&self.path, &rotated)?;
        self.file = BufWriter::new(open_append(&self.path)?);
        self.written = 0;
        self.write_resize_record()
    }
}

impl Drop for OutputLog {
    fn drop(&mut self) {
        let _ = self.file.flush();
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub fn rotated_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".1");
    path.with_file_name(name)
}

pub fn replay(path: &Path) -> io::Result<Option<vt100::Parser>> {
    let mut parser: Option<vt100::Parser> = None;
    let mut found = false;
    for file in [rotated_path(path), path.to_path_buf()] {
        match File::open(&file) {
            Ok(handle) => {
                found = true;
                replay_records(BufReader::new(handle), &mut parser)?;
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    if !found {
        return Ok(None);
    }
    Ok(Some(parser.unwrap_or_else(|| vt100::Parser::new(24, 80, REPLAY_SCROLLBACK))))
}

fn replay_records<R: Read>(mut reader: R, parser: &mut Option<vt100::Parser>) -> io::Result<()> {
    loop {
        let mut header = [0u8; 5];
        match reader.read_exact(&mut header) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(err) => return Err(err),
        }
        let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if len > MAX_RECORD_LEN {
            return Ok(());
        }
        let mut payload = vec![0u8; len];
        match reader.read_exact(&mut payload) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(err) => return Err(err),
        }
        match header[0] {
            TAG_RESIZE if payload.len() == 4 => {
                let size = TermSize::new(
                    u16::from_le_bytes([payload[0], payload[1]]),
                    u16::from_le_bytes([payload[2], payload[3]]),
                );
                match parser {
                    Some(p) => p.screen_mut().set_size(size.rows, size.cols),
                    None => *parser = Some(vt100::Parser::new(size.rows, size.cols, REPLAY_SCROLLBACK)),
                }
            }
            TAG_OUTPUT => parser.get_or_insert_with(|| vt100::Parser::new(24, 80, REPLAY_SCROLLBACK)).process(&payload),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orkai-oplog-{tag}-{}-{}", std::process::id(), crate::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("output.log")
    }

    #[test]
    fn replay_reconstructs_screen_and_size() {
        let path = temp_path("replay");
        {
            let mut log = OutputLog::open(path.clone(), TermSize { rows: 5, cols: 30 }).unwrap();
            log.output(b"hello\r\n").unwrap();
            log.resize(TermSize { rows: 6, cols: 40 }).unwrap();
            log.output(b"world").unwrap();
        }
        let parser = replay(&path).unwrap().unwrap();
        assert_eq!(parser.screen().size(), (6, 40));
        assert!(parser.screen().contents().contains("hello"));
        assert!(parser.screen().contents().contains("world"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn replay_tolerates_truncated_tail() {
        let path = temp_path("truncated");
        {
            let mut log = OutputLog::open(path.clone(), TermSize { rows: 5, cols: 30 }).unwrap();
            log.output(b"intact").unwrap();
        }
        let mut data = fs::read(&path).unwrap();
        data.extend_from_slice(&[TAG_OUTPUT, 50, 0, 0, 0, b'x']);
        fs::write(&path, data).unwrap();
        let parser = replay(&path).unwrap().unwrap();
        assert!(parser.screen().contents().contains("intact"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn rotation_keeps_previous_segment_for_replay() {
        let path = temp_path("rotate");
        {
            let mut log = OutputLog::open_with_limit(path.clone(), TermSize { rows: 5, cols: 30 }, 64).unwrap();
            log.output(b"first segment that is long enough to rotate the log\r\n").unwrap();
            log.output(b"second").unwrap();
        }
        assert!(rotated_path(&path).exists());
        let parser = replay(&path).unwrap().unwrap();
        let contents = parser.screen().contents();
        assert!(contents.contains("first segment"));
        assert!(contents.contains("second"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn replay_of_missing_log_is_none() {
        let path = temp_path("none");
        assert!(replay(&path).unwrap().is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn reopening_appends_to_existing_log() {
        let path = temp_path("append");
        {
            let mut log = OutputLog::open(path.clone(), TermSize { rows: 5, cols: 30 }).unwrap();
            log.output(b"before restart\r\n").unwrap();
        }
        {
            let mut log = OutputLog::open(path.clone(), TermSize { rows: 5, cols: 30 }).unwrap();
            log.output(b"after restart").unwrap();
        }
        let contents = replay(&path).unwrap().unwrap().screen().contents();
        assert!(contents.contains("before restart"));
        assert!(contents.contains("after restart"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
