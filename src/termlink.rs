use std::cmp::Reverse;
use std::collections::VecDeque;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

pub const LINK_RECORD_OSC: &str = "7502";
const MAX_LINKS: usize = 256;
const HYPERLINK_OSC: &[u8] = b"8";
const MAX_OSC_BYTES: usize = 4096;
const MAX_TEXT_BYTES: usize = 1024;
const MAX_URL_BYTES: usize = 2048;
const MIN_TEXT_CHARS: usize = 2;
const WEB_SCHEMES: [&str; 2] = ["http://", "https://"];
const URL_SYMBOLS: &str = "-._~:/?#[]@!$&'()*+,;=%";
const URL_TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"'];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Link {
    text: String,
    url: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Scan {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

#[derive(Debug, Default)]
pub struct LinkTracker {
    scan: Scan,
    osc: Vec<u8>,
    discard: bool,
    url: Option<String>,
    text: Vec<u8>,
    links: VecDeque<Link>,
}

impl LinkTracker {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.step(byte);
        }
    }

    pub fn records(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for link in &self.links {
            let record =
                format!("\x1b]{LINK_RECORD_OSC};{};{}\x07", STANDARD.encode(&link.url), STANDARD.encode(&link.text));
            out.extend_from_slice(record.as_bytes());
        }
        out
    }

    pub fn link_at(&self, screen: &vt100::Screen, row: u16, col: u16) -> Option<String> {
        let line = LogicalLine::around(screen, row);
        let at = line.offset_of(row, col)?;
        self.hyperlink_at(&line.text, at).or_else(|| url_at(&line.text, at))
    }

    fn hyperlink_at(&self, text: &str, at: usize) -> Option<String> {
        self.links
            .iter()
            .rev()
            .filter(|link| {
                text.match_indices(link.text.as_str()).any(|(start, found)| {
                    let end = start + found.len();
                    start <= at && at < end && stands_alone(text, start, end)
                })
            })
            .min_by_key(|link| Reverse(link.text.len()))
            .map(|link| link.url.clone())
    }

    fn step(&mut self, byte: u8) {
        match self.scan {
            Scan::Ground => match byte {
                0x1b => self.scan = Scan::Escape,
                0x00..=0x1f | 0x7f => self.end_text(),
                _ => self.push_text(byte),
            },
            Scan::Escape => match byte {
                b'[' => self.scan = Scan::Csi,
                b']' => self.start_string(false),
                b'P' | b'X' | b'^' | b'_' => self.start_string(true),
                0x20..=0x2f => {}
                _ => {
                    self.end_text();
                    self.scan = Scan::Ground;
                }
            },
            Scan::Csi => {
                if (0x40..=0x7e).contains(&byte) {
                    if byte != b'm' {
                        self.end_text();
                    }
                    self.scan = Scan::Ground;
                }
            }
            Scan::Osc => match byte {
                0x07 => self.finish_osc(),
                0x1b => self.scan = Scan::OscEscape,
                _ if self.discard || self.osc.len() >= MAX_OSC_BYTES => self.discard = true,
                _ => self.osc.push(byte),
            },
            Scan::OscEscape => {
                if byte == b'\\' {
                    self.finish_osc();
                } else {
                    self.scan = Scan::Escape;
                    self.step(byte);
                }
            }
        }
    }

    fn start_string(&mut self, discard: bool) {
        self.osc.clear();
        self.discard = discard;
        self.scan = Scan::Osc;
    }

    fn finish_osc(&mut self) {
        self.scan = Scan::Ground;
        if self.discard {
            return;
        }
        let osc = std::mem::take(&mut self.osc);
        let mut fields = osc.splitn(3, |&b| b == b';');
        let code = fields.next().unwrap_or_default();
        if code == HYPERLINK_OSC {
            let url = fields.nth(1).unwrap_or_default();
            self.end_text();
            self.url = std::str::from_utf8(url).ok().filter(|url| is_web_link(url)).map(str::to_string);
        } else if code == LINK_RECORD_OSC.as_bytes() {
            let decode = |field: Option<&[u8]>| STANDARD.decode(field?).ok().and_then(|b| String::from_utf8(b).ok());
            if let (Some(url), Some(text)) = (decode(fields.next()), decode(fields.next())) {
                self.remember(text, url);
            }
        }
    }

    fn push_text(&mut self, byte: u8) {
        if self.url.is_some() && self.text.len() < MAX_TEXT_BYTES {
            self.text.push(byte);
        }
    }

    fn end_text(&mut self) {
        let text = std::mem::take(&mut self.text);
        if let Some(url) = self.url.clone() {
            self.remember(String::from_utf8_lossy(&text).trim().to_string(), url);
        }
    }

    fn remember(&mut self, text: String, url: String) {
        if text.chars().filter(|c| !c.is_whitespace()).count() < MIN_TEXT_CHARS || !is_web_link(&url) {
            return;
        }
        let link = Link { text, url };
        if let Some(index) = self.links.iter().position(|known| *known == link) {
            self.links.remove(index);
        }
        self.links.push_back(link);
        while self.links.len() > MAX_LINKS {
            self.links.pop_front();
        }
    }
}

pub fn is_web_link(url: &str) -> bool {
    url.len() <= MAX_URL_BYTES
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && WEB_SCHEMES.iter().any(|scheme| {
            url.get(..scheme.len()).is_some_and(|head| head.eq_ignore_ascii_case(scheme)) && url.len() > scheme.len()
        })
}

struct LogicalLine {
    text: String,
    cells: Vec<(u16, u16, usize)>,
}

impl LogicalLine {
    fn around(screen: &vt100::Screen, row: u16) -> Self {
        let (rows, cols) = screen.size();
        let mut first = row;
        while first > 0 && screen.row_wrapped(first - 1) {
            first -= 1;
        }
        let mut last = row;
        while last + 1 < rows && screen.row_wrapped(last) {
            last += 1;
        }
        let mut line = Self { text: String::new(), cells: Vec::new() };
        for r in first..=last {
            for c in 0..cols {
                let Some(cell) = screen.cell(r, c) else { continue };
                if cell.is_wide_continuation() {
                    continue;
                }
                line.cells.push((r, c, line.text.len()));
                if cell.has_contents() {
                    line.text.push_str(cell.contents());
                } else {
                    line.text.push(' ');
                }
            }
        }
        line
    }

    fn offset_of(&self, row: u16, col: u16) -> Option<usize> {
        self.cells.iter().rev().find(|(r, c, _)| (*r, *c) <= (row, col)).map(|(_, _, offset)| *offset)
    }
}

fn stands_alone(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

fn url_at(text: &str, at: usize) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    WEB_SCHEMES
        .iter()
        .flat_map(|scheme| lower.match_indices(scheme).map(|(start, _)| start))
        .map(|start| (start, start + url_len(&text[start..])))
        .find(|(start, end)| *start <= at && at < *end)
        .map(|(start, end)| text[start..end].to_string())
        .filter(|url| is_web_link(url))
}

fn url_len(rest: &str) -> usize {
    let mut end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || URL_SYMBOLS.contains(c))).unwrap_or(rest.len());
    while let Some(last) = rest[..end].chars().next_back() {
        let url = &rest[..end];
        let unbalanced =
            |open: char, close: char| last == close && url.matches(open).count() < url.matches(close).count();
        if URL_TRAILING.contains(&last) || unbalanced('(', ')') || unbalanced('[', ']') {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCS: &str = "https://docs.example.com/guide?a=1&b=2";

    fn screen(rows: u16, cols: u16, bytes: &[u8]) -> vt100::Parser {
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(bytes);
        parser
    }

    fn tracked(bytes: &[u8]) -> (LinkTracker, vt100::Parser) {
        let mut tracker = LinkTracker::default();
        tracker.feed(bytes);
        (tracker, screen(4, 40, bytes))
    }

    fn hyperlink(url: &str, text: &str) -> Vec<u8> {
        format!("\x1b]8;id=x;{url}\x1b\\{text}\x1b]8;;\x1b\\").into_bytes()
    }

    #[test]
    fn a_titled_hyperlink_opens_where_its_title_is_shown() {
        let bytes = [b"see ".as_slice(), &hyperlink(DOCS, "the guide"), b" now"].concat();
        let (tracker, parser) = tracked(&bytes);
        for col in 4..13 {
            assert_eq!(tracker.link_at(parser.screen(), 0, col).as_deref(), Some(DOCS), "col {col}");
        }
        assert_eq!(tracker.link_at(parser.screen(), 0, 2), None);
        assert_eq!(tracker.link_at(parser.screen(), 0, 14), None);
    }

    #[test]
    fn hyperlinks_split_across_writes_and_chunks_are_still_found() {
        let bytes = [
            b"\x1b]8;id=a;".as_slice(),
            DOCS.as_bytes(),
            b"\x07\x1b[1;1H\x1b[1mhttps://docs\x1b[m\x1b[2;1Hexample.com/guide\x1b]8;;\x07",
        ]
        .concat();
        let mut tracker = LinkTracker::default();
        for chunk in bytes.chunks(3) {
            tracker.feed(chunk);
        }
        let parser = screen(4, 40, &bytes);
        assert_eq!(tracker.links.len(), 2, "a cursor move ends a part, an SGR does not");
        assert_eq!(tracker.link_at(parser.screen(), 0, 3).as_deref(), Some(DOCS));
        assert_eq!(tracker.link_at(parser.screen(), 1, 10).as_deref(), Some(DOCS));
    }

    #[test]
    fn a_hyperlink_title_inside_a_longer_word_is_not_a_link() {
        let bytes = [hyperlink(DOCS, "guide"), b"\r\nguidebook guide".to_vec()].concat();
        let (tracker, parser) = tracked(&bytes);
        assert_eq!(tracker.link_at(parser.screen(), 1, 2), None);
        assert_eq!(tracker.link_at(parser.screen(), 1, 12).as_deref(), Some(DOCS));
    }

    #[test]
    fn the_longest_and_then_newest_title_wins() {
        let older = "https://example.com/older";
        let newer = "https://example.com/newer";
        let long = "https://example.com/long";
        let bytes = [hyperlink(older, "docs"), hyperlink(newer, "docs"), hyperlink(long, "the docs")].concat();
        let mut tracker = LinkTracker::default();
        tracker.feed(&bytes);
        let parser = screen(2, 40, b"docs | the docs");
        assert_eq!(tracker.link_at(parser.screen(), 0, 1).as_deref(), Some(newer));
        assert_eq!(tracker.link_at(parser.screen(), 0, 12).as_deref(), Some(long));
    }

    #[test]
    fn only_web_links_are_kept_and_opened() {
        let bytes = [
            hyperlink("file:///C:/repo/src/main.rs", "src/main.rs"),
            hyperlink("javascript:alert(1)", "click me"),
            b" file:///etc/passwd".to_vec(),
        ]
        .concat();
        let (tracker, parser) = tracked(&bytes);
        assert!(tracker.links.is_empty());
        assert_eq!(tracker.link_at(parser.screen(), 0, 3), None);
        assert_eq!(tracker.link_at(parser.screen(), 0, 22), None);
        assert!(is_web_link("HTTPS://Example.com"));
        assert!(!is_web_link("https://"));
        assert!(!is_web_link("https://example.com/a b"));
        assert!(!is_web_link(&format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES))));
    }

    #[test]
    fn plain_urls_open_without_trailing_punctuation_and_across_wrapped_rows() {
        let parser = screen(4, 20, b"(see https://example.com/a_(b).) and https://example.com/very/long/path");
        let tracker = LinkTracker::default();
        let screen = parser.screen();
        let first = Some("https://example.com/a_(b)");
        let second = Some("https://example.com/very/long/path");
        for (row, col, expected) in [
            (0, 2, None),
            (0, 5, first),
            (0, 19, first),
            (1, 9, first),
            (1, 10, None),
            (1, 11, None),
            (1, 13, None),
            (1, 17, second),
            (2, 0, second),
            (3, 10, second),
            (3, 11, None),
        ] {
            assert_eq!(tracker.link_at(screen, row, col).as_deref(), expected, "row {row} col {col}");
        }
    }

    #[test]
    fn records_carry_the_links_to_a_fresh_tracker() {
        let bytes = [hyperlink(DOCS, "the guide"), hyperlink("https://example.com/ünï", "título ✓")].concat();
        let (tracker, _) = tracked(&bytes);
        let mut replica = LinkTracker::default();
        replica.feed(&tracker.records());
        assert_eq!(replica.links, tracker.links);
        let mut parser = screen(2, 40, b"the guide");
        parser.process(&tracker.records());
        assert_eq!(parser.screen().contents(), "the guide", "records draw nothing");
        assert_eq!(replica.link_at(parser.screen(), 0, 5).as_deref(), Some(DOCS));
    }

    #[test]
    fn links_are_bounded_and_repeats_move_to_the_end() {
        let mut tracker = LinkTracker::default();
        for i in 0..MAX_LINKS + 10 {
            tracker.feed(&hyperlink(&format!("https://example.com/{i}"), &format!("link {i}")));
        }
        assert_eq!(tracker.links.len(), MAX_LINKS);
        assert_eq!(tracker.links.front().unwrap().text, "link 10");
        tracker.feed(&hyperlink("https://example.com/10", "link 10"));
        assert_eq!(tracker.links.len(), MAX_LINKS);
        assert_eq!(tracker.links.back().unwrap().text, "link 10");
    }

    #[test]
    fn other_strings_and_oversized_sequences_leave_no_links() {
        let huge = format!("\x1b]8;;https://example.com/{}\x07huge\x1b]8;;\x07", "a".repeat(MAX_OSC_BYTES));
        let bytes = [
            b"\x1b]8;;https://example.com/x\x07\x1bPq inside a dcs\x1b\\\x1b]8;;\x07".to_vec(),
            huge.into_bytes(),
            hyperlink("https://example.com/x", " x "),
        ]
        .concat();
        let mut tracker = LinkTracker::default();
        tracker.feed(&bytes);
        assert!(tracker.links.is_empty(), "{:?}", tracker.links);
    }
}
