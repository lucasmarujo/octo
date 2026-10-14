pub const MAX_COMMAND_CHARS: usize = 200;

#[derive(Default, Debug)]
pub struct LineTracker {
    current: String,
    last: Option<String>,
}

impl LineTracker {
    pub fn feed(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => skip_escape(&mut chars),
                '\r' | '\n' => self.commit(),
                '\x7f' | '\x08' => {
                    self.current.pop();
                }
                '\x03' | '\x15' => self.current.clear(),
                c if c.is_control() => {}
                c => {
                    if self.current.chars().count() < MAX_COMMAND_CHARS {
                        self.current.push(c);
                    }
                }
            }
        }
    }

    pub fn set_last(&mut self, command: &str) {
        let trimmed = command.trim();
        if !trimmed.is_empty() {
            self.last = Some(trimmed.chars().take(MAX_COMMAND_CHARS).collect());
        }
    }

    pub fn last(&self) -> Option<&str> {
        self.last.as_deref()
    }

    fn commit(&mut self) {
        let line = std::mem::take(&mut self.current);
        self.set_last(&line);
    }
}

fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.peek() {
        Some('[') | Some('O') => {
            chars.next();
            while let Some(&c) = chars.peek() {
                chars.next();
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        }
        Some(_) => {
            chars.next();
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_submitted_commands() {
        let mut t = LineTracker::default();
        t.feed(b"npm run dev\r");
        assert_eq!(t.last(), Some("npm run dev"));
    }

    #[test]
    fn handles_backspace_and_split_writes() {
        let mut t = LineTracker::default();
        t.feed(b"car");
        t.feed(b"gox\x7f");
        t.feed(b" test\r");
        assert_eq!(t.last(), Some("cargo test"));
    }

    #[test]
    fn ignores_escape_sequences_and_empty_lines() {
        let mut t = LineTracker::default();
        t.feed(b"ls\r");
        t.feed(b"\x1b[A\x1b[B\r");
        assert_eq!(t.last(), Some("ls"));
        t.feed(b"\x1b[200~git status\x1b[201~\r");
        assert_eq!(t.last(), Some("git status"));
    }

    #[test]
    fn ctrl_c_discards_partial_line() {
        let mut t = LineTracker::default();
        t.feed(b"rm -rf build\x03\r");
        assert_eq!(t.last(), None);
    }

    #[test]
    fn long_lines_are_bounded() {
        let mut t = LineTracker::default();
        t.feed(&vec![b'a'; MAX_COMMAND_CHARS * 3]);
        t.feed(b"\r");
        assert_eq!(t.last().unwrap().len(), MAX_COMMAND_CHARS);
    }
}
