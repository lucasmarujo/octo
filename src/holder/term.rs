use crate::holder::detect::{self, Detector, Signals, TAIL_ROWS};
use crate::holder::input::LineTracker;
use crate::model::{AgentKind, ExitInfo, StatusReport};
use crate::protocol::TermSize;
use crate::termfmt;

pub const HOLDER_SCROLLBACK: usize = 1_000;
pub const INPUT_QUIET_MS: u64 = 900;
pub const INPUT_NO_OUTPUT_TIMEOUT_MS: u64 = 20_000;
pub const ACTIVITY_MAX_CHARS: usize = 160;
pub const SUBMIT_MIN_MS: u64 = 500;
pub const SUBMIT_QUIET_MS: u64 = 350;
pub const SUBMIT_MAX_MS: u64 = 3_000;

#[derive(Default)]
pub struct Responder {
    pub title: String,
    pub responses: Vec<u8>,
}

impl vt100::Callbacks for Responder {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).into_owned();
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        let (row, col) = screen.cursor_position();
        let (rows, cols) = screen.size();
        match (i1, c, first) {
            (None, 'n', 5) => self.responses.extend_from_slice(b"\x1b[0n"),
            (None, 'n', 6) => self.responses.extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes()),
            (Some(b'?'), 'n', 6) => {
                self.responses.extend_from_slice(format!("\x1b[?{};{}R", row + 1, col + 1).as_bytes())
            }
            (None, 'c', 0) => self.responses.extend_from_slice(b"\x1b[?1;2c"),
            (Some(b'>'), 'c', 0) => self.responses.extend_from_slice(b"\x1b[>0;10;1c"),
            (None, 't', 18) => self.responses.extend_from_slice(format!("\x1b[8;{rows};{cols}t").as_bytes()),
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            [b"10", b"?"] => self.responses.extend_from_slice(b"\x1b]10;rgb:d0d0/d0d0/d0d0\x1b\\"),
            [b"11", b"?"] => self.responses.extend_from_slice(b"\x1b]11;rgb:1616/1616/1e1e\x1b\\"),
            _ => {}
        }
    }
}

pub struct TermState {
    parser: vt100::Parser<Responder>,
    kind: AgentKind,
    detector: Detector,
    signals: Signals,
    dirty: bool,
    started_at: u64,
    last_output_at: u64,
    tracker: LineTracker,
    task: String,
    pending_input: Option<String>,
    submit_after: Option<u64>,
}

impl TermState {
    pub fn new(kind: AgentKind, size: TermSize, started_at: u64, task: String, initial_input: Option<String>) -> Self {
        let pending_input = initial_input.filter(|text| !text.trim().is_empty());
        let mut tracker = LineTracker::default();
        if kind == AgentKind::Terminal
            && let Some(command) = &pending_input
        {
            tracker.set_last(command);
        }
        Self {
            parser: vt100::Parser::new_with_callbacks(size.rows, size.cols, HOLDER_SCROLLBACK, Responder::default()),
            kind,
            detector: Detector::new(started_at),
            signals: Signals::default(),
            dirty: false,
            started_at,
            last_output_at: 0,
            tracker,
            task,
            pending_input,
            submit_after: None,
        }
    }

    pub fn process(&mut self, bytes: &[u8], now: u64) -> Vec<u8> {
        self.parser.process(bytes);
        self.last_output_at = now;
        self.dirty = true;
        std::mem::take(&mut self.parser.callbacks_mut().responses)
    }

    pub fn size(&self) -> TermSize {
        let (rows, cols) = self.parser.screen().size();
        TermSize { rows, cols }
    }

    pub fn resize(&mut self, size: TermSize) {
        self.parser.screen_mut().set_size(size.rows, size.cols);
        self.dirty = true;
    }

    pub fn title(&self) -> &str {
        &self.parser.callbacks().title
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn note_input(&mut self, bytes: &[u8]) {
        if self.kind == AgentKind::Terminal {
            self.tracker.feed(bytes);
        }
    }

    pub fn mark_seen(&mut self) -> bool {
        self.detector.mark_seen()
    }

    pub fn evaluate(&mut self, now: u64) -> bool {
        if self.dirty {
            self.dirty = false;
            let tail = termfmt::screen_tail(self.parser.screen(), TAIL_ROWS);
            self.signals = detect::analyze(self.kind, self.title(), &tail);
        }
        self.detector.observe(self.kind, self.signals, self.last_output_at, now)
    }

    pub fn activity(&self) -> String {
        let from_title = detect::activity_from_title(self.kind, self.title());
        let text = match self.kind {
            AgentKind::Terminal => self.tracker.last().map(str::to_string).or(from_title).unwrap_or_default(),
            _ => from_title.unwrap_or_else(|| self.task.clone()),
        };
        let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        single_line.chars().take(ACTIVITY_MAX_CHARS).collect()
    }

    pub fn report(&self, exit: Option<ExitInfo>) -> StatusReport {
        let size = self.size();
        StatusReport {
            state: self.detector.state(),
            state_since_ms: self.detector.since(),
            turn_started_ms: self.detector.turn_started(),
            last_turn_ms: self.detector.last_turn_ms(),
            started_at_ms: self.started_at,
            title: self.title().to_string(),
            activity: self.activity(),
            rows: size.rows,
            cols: size.cols,
            exit,
        }
    }

    pub fn take_ready_input(&mut self, now: u64) -> Option<Vec<u8>> {
        self.pending_input.as_ref()?;
        if self.signals.waiting || self.signals.busy {
            return None;
        }
        let quiet = self.last_output_at > 0 && now.saturating_sub(self.last_output_at) >= INPUT_QUIET_MS;
        let silent_too_long =
            self.last_output_at == 0 && now.saturating_sub(self.started_at) >= INPUT_NO_OUTPUT_TIMEOUT_MS;
        if !(quiet || silent_too_long) {
            return None;
        }
        let text = self.pending_input.take()?;
        self.submit_after = Some(now);
        Some(encode_typed_text(&text, self.parser.screen().bracketed_paste()))
    }

    pub fn take_submit(&mut self, now: u64) -> bool {
        let Some(typed_at) = self.submit_after else {
            return false;
        };
        let elapsed = now.saturating_sub(typed_at);
        let quiet = now.saturating_sub(self.last_output_at) >= SUBMIT_QUIET_MS;
        if (elapsed >= SUBMIT_MIN_MS && quiet) || elapsed >= SUBMIT_MAX_MS {
            self.submit_after = None;
            return true;
        }
        false
    }

    pub fn snapshot(&mut self) -> Vec<u8> {
        termfmt::snapshot(self.parser.screen_mut())
    }
}

pub fn encode_typed_text(text: &str, bracketed_paste: bool) -> Vec<u8> {
    let trimmed = text.trim_end_matches(['\r', '\n']);
    if trimmed.contains('\n') && bracketed_paste {
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(trimmed.replace("\r\n", "\n").as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        return out;
    }
    trimmed.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentState;

    fn state(kind: AgentKind) -> TermState {
        TermState::new(kind, TermSize { rows: 6, cols: 40 }, 0, "write the docs".into(), None)
    }

    #[test]
    fn answers_cursor_position_query_at_query_time() {
        let mut t = state(AgentKind::Codex);
        let replies = t.process(b"abc\x1b[6nxyz", 1);
        assert_eq!(replies, b"\x1b[1;4R");
    }

    #[test]
    fn answers_device_attributes_and_status() {
        let mut t = state(AgentKind::Codex);
        assert_eq!(t.process(b"\x1b[c", 1), b"\x1b[?1;2c");
        assert_eq!(t.process(b"\x1b[5n", 1), b"\x1b[0n");
        assert_eq!(t.process(b"\x1b[>c", 1), b"\x1b[>0;10;1c");
        assert_eq!(t.process(b"\x1b[18t", 1), b"\x1b[8;6;40t");
    }

    #[test]
    fn answers_color_queries() {
        let mut t = state(AgentKind::Codex);
        let reply = t.process(b"\x1b]11;?\x1b\\", 1);
        assert!(reply.starts_with(b"\x1b]11;rgb:"));
    }

    #[test]
    fn captures_window_title() {
        let mut t = state(AgentKind::Claude);
        t.process(b"\x1b]0;\xe2\x9c\xb3 Shepherd side panel\x07", 1);
        assert_eq!(t.title(), "✳ Shepherd side panel");
        assert_eq!(t.activity(), "Shepherd side panel");
    }

    #[test]
    fn activity_falls_back_to_task_for_ai_agents() {
        let t = state(AgentKind::Claude);
        assert_eq!(t.activity(), "write the docs");
    }

    #[test]
    fn terminal_activity_is_last_command() {
        let mut t = TermState::new(
            AgentKind::Terminal,
            TermSize { rows: 6, cols: 40 },
            0,
            String::new(),
            Some("npm run dev".into()),
        );
        assert_eq!(t.activity(), "npm run dev");
        t.note_input(b"cargo test\r");
        assert_eq!(t.activity(), "cargo test");
    }

    #[test]
    fn evaluate_detects_working_from_screen() {
        let mut t = state(AgentKind::Claude);
        t.process(b"thinking (esc to interrupt)", 100);
        assert!(t.evaluate(150));
        assert_eq!(t.report(None).state, AgentState::Working);
    }

    #[test]
    fn initial_input_waits_for_quiet_output() {
        let mut t = TermState::new(
            AgentKind::Claude,
            TermSize { rows: 6, cols: 40 },
            0,
            "fix".into(),
            Some("fix the bug".into()),
        );
        t.process(b"> ", 1_000);
        t.evaluate(1_000);
        assert!(t.take_ready_input(1_200).is_none());
        let typed_at = 1_000 + INPUT_QUIET_MS;
        let typed = t.take_ready_input(typed_at).unwrap();
        assert_eq!(typed, b"fix the bug");
        assert!(t.take_ready_input(10_000).is_none());
        assert!(!t.take_submit(typed_at + 100));
        t.process(b"fix the bug", typed_at + 200);
        assert!(!t.take_submit(typed_at + SUBMIT_MIN_MS));
        assert!(t.take_submit(typed_at + 200 + SUBMIT_QUIET_MS));
        assert!(!t.take_submit(typed_at + 10_000));
    }

    #[test]
    fn submit_is_forced_when_output_never_settles() {
        let mut t = TermState::new(AgentKind::Codex, TermSize { rows: 6, cols: 40 }, 0, "x".into(), Some("x".into()));
        t.process(b"ready", 1_000);
        t.evaluate(1_000);
        let typed = t.take_ready_input(5_000).unwrap();
        assert_eq!(typed, b"x");
        t.process(b"spinner", 5_000 + SUBMIT_MAX_MS - 1);
        assert!(!t.take_submit(5_000 + SUBMIT_MAX_MS - 1));
        assert!(t.take_submit(5_000 + SUBMIT_MAX_MS));
    }

    #[test]
    fn initial_input_is_held_while_waiting_for_approval() {
        let mut t =
            TermState::new(AgentKind::Claude, TermSize { rows: 6, cols: 40 }, 0, "fix".into(), Some("fix".into()));
        t.process(b"Do you trust the files in this folder?", 1_000);
        t.evaluate(1_000);
        assert!(t.take_ready_input(60_000).is_none());
    }

    #[test]
    fn initial_input_is_sent_when_program_stays_silent() {
        let mut t =
            TermState::new(AgentKind::Terminal, TermSize { rows: 6, cols: 40 }, 0, String::new(), Some("ls".into()));
        assert!(t.take_ready_input(1_000).is_none());
        assert_eq!(t.take_ready_input(INPUT_NO_OUTPUT_TIMEOUT_MS).unwrap(), b"ls");
    }

    #[test]
    fn multi_line_text_uses_bracketed_paste_when_enabled() {
        assert_eq!(encode_typed_text("a\nb\n", true), b"\x1b[200~a\nb\x1b[201~");
        assert_eq!(encode_typed_text("a\nb\n", false), b"a b");
        assert_eq!(encode_typed_text("single", true), b"single");
    }

    #[test]
    fn resize_changes_reported_size() {
        let mut t = state(AgentKind::Terminal);
        t.resize(TermSize { rows: 30, cols: 100 });
        let report = t.report(None);
        assert_eq!((report.rows, report.cols), (30, 100));
    }
}
