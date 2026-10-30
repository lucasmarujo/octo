use crate::holder::detect::{self, Detector, Signals, TAIL_ROWS};
use crate::holder::input::LineTracker;
use crate::hooks::{self, HookSignal};
use crate::model::{AgentKind, ExitInfo, NestedAgent, StatusReport};
use crate::protocol::TermSize;
use crate::termfmt;
use crate::termlink::LinkTracker;

pub const HOLDER_SCROLLBACK: usize = 1_000;
pub const INPUT_QUIET_MS: u64 = 900;
pub const INPUT_NO_OUTPUT_TIMEOUT_MS: u64 = 20_000;
pub const INPUT_READY_TIMEOUT_MS: u64 = 20_000;
pub const ACTIVITY_MAX_CHARS: usize = 160;
pub const SUBMIT_MIN_MS: u64 = 500;
pub const SUBMIT_QUIET_MS: u64 = 350;
pub const SUBMIT_MAX_MS: u64 = 3_000;
pub const SUBMIT_NO_ECHO_TIMEOUT_MS: u64 = 20_000;

#[derive(Default)]
pub struct Responder {
    pub title: String,
    pub responses: Vec<u8>,
    pub signals: Vec<HookSignal>,
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
            [code, report @ ..] if *code == hooks::PROGRAM_STATUS_OSC.as_bytes() => {
                let report: Vec<String> =
                    report.iter().map(|part| String::from_utf8_lossy(part).into_owned()).collect();
                self.signals.extend(hooks::program_status_signal(&report.join(";")));
            }
            _ => {}
        }
    }
}

pub struct TermState {
    parser: vt100::Parser<Responder>,
    links: LinkTracker,
    kind: AgentKind,
    nested: Option<NestedAgent>,
    detector: Detector,
    signals: Signals,
    dirty: bool,
    started_at: u64,
    last_output_at: u64,
    tracker: LineTracker,
    task: String,
    pending_input: Option<String>,
    submit_after: Option<u64>,
    echoed_at: Option<u64>,
    input_at: u64,
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
            links: LinkTracker::default(),
            kind,
            nested: None,
            detector: Detector::new(started_at),
            signals: Signals::default(),
            dirty: false,
            started_at,
            last_output_at: 0,
            tracker,
            task,
            pending_input,
            submit_after: None,
            echoed_at: None,
            input_at: 0,
        }
    }

    pub fn process(&mut self, bytes: &[u8], now: u64) -> Vec<u8> {
        self.parser.process(bytes);
        self.links.feed(bytes);
        for signal in std::mem::take(&mut self.parser.callbacks_mut().signals) {
            self.apply_hook(signal, now);
        }
        self.last_output_at = now;
        if self.echoed_at.is_none() && self.submit_after.is_some_and(|typed_at| now >= typed_at) {
            self.echoed_at = Some(now);
        }
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

    pub fn running_kind(&self) -> AgentKind {
        self.nested.as_ref().map_or(self.kind, |nested| nested.kind)
    }

    pub fn set_nested(&mut self, found: Option<NestedAgent>, now: u64) -> bool {
        let found = found.map(|mut nested| {
            if let Some(current) = self.nested.as_ref().filter(|current| current.pid == nested.pid) {
                nested.since_ms = current.since_ms;
            }
            nested
        });
        if self.nested == found {
            return false;
        }
        let program_changed = self.nested.as_ref().map(|n| n.pid) != found.as_ref().map(|n| n.pid);
        self.nested = found;
        if program_changed {
            self.detector = Detector::new(now);
            self.dirty = true;
        }
        true
    }

    pub fn note_input(&mut self, bytes: &[u8], now: u64) {
        self.input_at = now;
        if self.kind == AgentKind::Terminal {
            self.tracker.feed(bytes);
        }
        if is_answer(bytes) {
            self.detector.user_answered();
        }
    }

    pub fn apply_hook(&mut self, signal: HookSignal, at: u64) -> bool {
        if self.nested.is_some() || !self.kind.is_ai() {
            return false;
        }
        self.detector.hook(signal, at)
    }

    pub fn mark_seen(&mut self) -> bool {
        self.detector.mark_seen()
    }

    pub fn evaluate(&mut self, now: u64) -> bool {
        if self.dirty {
            self.dirty = false;
            let tail = termfmt::screen_tail(self.parser.screen(), TAIL_ROWS);
            self.signals = detect::analyze(self.running_kind(), self.title(), &tail);
        }
        self.detector.observe(self.running_kind(), self.signals, self.last_output_at, now)
    }

    pub fn activity(&self) -> String {
        let from_title = detect::activity_from_title(self.kind, self.title());
        let last_command = self.tracker.last().map(str::to_string);
        let text = match (self.kind, self.nested.as_ref()) {
            (AgentKind::Terminal, Some(nested)) => {
                detect::activity_from_title(nested.kind, self.title()).or(last_command).unwrap_or_default()
            }
            (AgentKind::Terminal, None) => last_command.or(from_title).unwrap_or_default(),
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
            nested: self.nested.clone(),
            viewers: None,
            test: None,
            input_at_ms: self.input_at,
        }
    }

    pub fn take_ready_input(&mut self, now: u64) -> Option<Vec<u8>> {
        self.pending_input.as_ref()?;
        if self.signals.waiting || self.signals.busy || !self.input_ready(now) {
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

    fn input_ready(&self, now: u64) -> bool {
        if !self.kind.is_ai() {
            return true;
        }
        let screen = self.parser.screen();
        let drawn = screen.bracketed_paste() && !screen.contents().trim().is_empty();
        drawn || now.saturating_sub(self.started_at) >= INPUT_READY_TIMEOUT_MS
    }

    pub fn take_submit(&mut self, now: u64) -> bool {
        let Some(typed_at) = self.submit_after else {
            return false;
        };
        let elapsed = now.saturating_sub(typed_at);
        let ready = match self.echoed_at {
            Some(echoed_at) => {
                let quiet = now.saturating_sub(self.last_output_at) >= SUBMIT_QUIET_MS;
                (elapsed >= SUBMIT_MIN_MS && quiet) || now.saturating_sub(echoed_at) >= SUBMIT_MAX_MS
            }
            None => elapsed >= SUBMIT_NO_ECHO_TIMEOUT_MS,
        };
        if ready {
            self.submit_after = None;
            self.echoed_at = None;
        }
        ready
    }

    pub fn snapshot(&mut self) -> Vec<u8> {
        let mut data = termfmt::snapshot(self.parser.screen_mut());
        data.extend_from_slice(&self.links.records());
        data
    }
}

fn is_answer(bytes: &[u8]) -> bool {
    bytes == [0x1b] || bytes.iter().any(|b| matches!(b, b'\r' | b'\n' | b'1'..=b'9'))
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
    fn snapshots_carry_the_links_the_agent_printed() {
        let mut t = state(AgentKind::Claude);
        t.process(b"read \x1b]8;id=1;https://example.com/guide\x1b\\the guide\x1b]8;;\x1b\\ first", 1);
        let data = t.snapshot();
        let mut replica = vt100::Parser::new(6, 40, 0);
        replica.process(&data);
        let mut links = LinkTracker::default();
        links.feed(&data);
        assert_eq!(replica.screen().contents(), "read the guide first");
        assert_eq!(links.link_at(replica.screen(), 0, 8).as_deref(), Some("https://example.com/guide"));
        assert_eq!(links.link_at(replica.screen(), 0, 2), None);
    }

    #[test]
    fn answers_cursor_position_query_at_query_time() {
        let mut t = state(AgentKind::Codex);
        let replies = t.process(b"abc\x1b[6nxyz", 1);
        assert_eq!(replies, b"\x1b[1;4R");
    }

    #[test]
    fn program_status_reports_drive_the_state_like_hooks() {
        let mut t = state(AgentKind::Antigravity);
        assert!(t.process(b"\x1b]7501;state=working:app=agy\x07", 1_000).is_empty());
        assert!(!t.evaluate(1_000));
        assert_eq!(t.report(None).state, AgentState::Working);
        t.process(b"\x1b]7501;state=blocked:kind=permission\x1b\\", 2_000);
        assert_eq!(t.report(None).state, AgentState::Paused);
        t.note_input(b"1", 2_500);
        t.process(b"\x1b]7501;state=working\x07", 3_000);
        assert_eq!(t.report(None).state, AgentState::Working);
        t.process(b"\x1b]7501;state=done:msg=ZG9uZQ==\x07", 9_000);
        let report = t.report(None);
        assert_eq!(report.state, AgentState::Done);
        assert_eq!(report.last_turn_ms, Some(8_000));
        t.process(b"\x1b]7501;?\x07\x1b]7501;state=clear\x07", 10_000);
        assert_eq!(t.report(None).state, AgentState::Done);

        let mut terminal = state(AgentKind::Terminal);
        terminal.process(b"\x1b]7501;state=working\x07", 1_000);
        assert_eq!(terminal.report(None).state, AgentState::Idle, "only ai agents follow status reports");
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
        t.note_input(b"cargo test\r", 10);
        assert_eq!(t.activity(), "cargo test");
    }

    #[test]
    fn the_report_says_when_someone_last_typed_into_the_agent() {
        let mut t = state(AgentKind::Claude);
        assert_eq!(t.report(None).input_at_ms, 0);
        t.note_input(b"fix the tests", 1_000);
        t.process(b"fix the tests", 1_100);
        assert_eq!(t.report(None).input_at_ms, 1_000);
        t.note_input(b"\r", 1_500);
        t.process(b"thinking (esc to interrupt)", 9_000);
        t.evaluate(9_100);
        assert_eq!(t.report(None).input_at_ms, 1_500, "output from the agent is not someone using it");
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
        t.process(b"\x1b[?2004h> ", 1_000);
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
        t.process(b"\x1b[?2004hready", 1_000);
        t.evaluate(1_000);
        let typed = t.take_ready_input(5_000).unwrap();
        assert_eq!(typed, b"x");
        let echoed_at = 5_100;
        t.process(b"x", echoed_at);
        t.process(b"spinner", echoed_at + SUBMIT_MAX_MS - 1);
        assert!(!t.take_submit(echoed_at + SUBMIT_MAX_MS - 1));
        assert!(t.take_submit(echoed_at + SUBMIT_MAX_MS));
    }

    #[test]
    fn submit_waits_until_a_busy_agent_echoes_the_typed_task() {
        let mut t = TermState::new(
            AgentKind::Claude,
            TermSize { rows: 6, cols: 40 },
            0,
            "fix".into(),
            Some("fix the bug".into()),
        );
        t.process(b"\x1b[?2004h> ", 1_000);
        t.evaluate(1_000);
        let typed_at = 1_000 + INPUT_QUIET_MS;
        assert_eq!(t.take_ready_input(typed_at).unwrap(), b"fix the bug");
        assert!(!t.take_submit(typed_at + SUBMIT_MIN_MS));
        assert!(!t.take_submit(typed_at + SUBMIT_MAX_MS));
        let echoed_at = typed_at + 5_000;
        t.process(b"fix the bug", echoed_at);
        assert!(!t.take_submit(echoed_at + SUBMIT_QUIET_MS - 1));
        assert!(t.take_submit(echoed_at + SUBMIT_QUIET_MS));
        assert!(!t.take_submit(echoed_at + SUBMIT_NO_ECHO_TIMEOUT_MS));
    }

    #[test]
    fn an_echo_in_the_same_millisecond_as_the_typing_still_counts() {
        let size = TermSize { rows: 6, cols: 40 };
        let mut t = TermState::new(AgentKind::Terminal, size, 0, String::new(), Some("echo hi".into()));
        t.process(b"C:\\work>", 1_000);
        t.evaluate(1_000);
        let typed_at = 1_000 + INPUT_QUIET_MS;
        assert_eq!(t.take_ready_input(typed_at).unwrap(), b"echo hi");
        t.process(b"echo hi", typed_at);
        assert!(!t.take_submit(typed_at + SUBMIT_QUIET_MS - 1));
        assert!(t.take_submit(typed_at + SUBMIT_MIN_MS), "enter goes in right after the echo, not after the fallback");
    }

    #[test]
    fn submit_falls_back_when_the_agent_never_echoes() {
        let mut t = TermState::new(AgentKind::Codex, TermSize { rows: 6, cols: 40 }, 0, "x".into(), Some("x".into()));
        t.process(b"\x1b[?2004hready", 1_000);
        t.evaluate(1_000);
        let typed_at = 1_000 + INPUT_QUIET_MS;
        assert_eq!(t.take_ready_input(typed_at).unwrap(), b"x");
        assert!(!t.take_submit(typed_at + SUBMIT_NO_ECHO_TIMEOUT_MS - 1));
        assert!(t.take_submit(typed_at + SUBMIT_NO_ECHO_TIMEOUT_MS));
    }

    #[test]
    fn initial_input_waits_until_the_agent_input_is_drawn() {
        let mut t = TermState::new(
            AgentKind::Claude,
            TermSize { rows: 6, cols: 40 },
            0,
            "fix".into(),
            Some("fix the bug".into()),
        );
        t.process(b"\x1b[?2004h\x1b[?u\x1b]7501;?\x07", 100);
        t.process(b"\x1b[<u\x1b[?2031l\x1b[?2004l", 300);
        t.evaluate(300);
        assert!(t.take_ready_input(300 + INPUT_QUIET_MS * 4).is_none());
        t.process(b"\x1b[?2004h\x1b[?1049h\x1b[2J Claude Code\r\n> ", 5_000);
        t.evaluate(5_000);
        assert!(t.take_ready_input(5_000 + INPUT_QUIET_MS - 1).is_none());
        assert_eq!(t.take_ready_input(5_000 + INPUT_QUIET_MS).unwrap(), b"fix the bug");
    }

    #[test]
    fn initial_input_falls_back_when_bracketed_paste_is_never_enabled() {
        let mut t =
            TermState::new(AgentKind::Codex, TermSize { rows: 6, cols: 40 }, 0, "fix".into(), Some("fix".into()));
        t.process(b"> ", 1_000);
        t.evaluate(1_000);
        assert!(t.take_ready_input(1_000 + INPUT_QUIET_MS).is_none());
        assert_eq!(t.take_ready_input(INPUT_READY_TIMEOUT_MS).unwrap(), b"fix");
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
    fn initial_input_waits_for_the_bypass_mode_warning_to_be_accepted() {
        let mut t =
            TermState::new(AgentKind::Claude, TermSize { rows: 8, cols: 80 }, 0, "fix".into(), Some("fix".into()));
        t.process(b"\x1b[?2004h WARNING: Claude Code running in Bypass Permissions mode\r\n \xe2\x9d\xaf 1. No, exit\r\n   2. Yes, I accept", 1_000);
        t.evaluate(1_000);
        assert!(t.take_ready_input(60_000).is_none(), "an enter typed here would pick No, exit");
        t.process(b"\x1b[2J\x1b[H> \r\n  bypass permissions on", 61_000);
        t.evaluate(61_000);
        assert_eq!(t.take_ready_input(61_000 + INPUT_QUIET_MS).unwrap(), b"fix");
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

    fn nested(kind: AgentKind, pid: u32, since_ms: u64) -> Option<NestedAgent> {
        Some(NestedAgent { kind, pid, conversation_id: None, cwd: None, since_ms })
    }

    #[test]
    fn a_terminal_running_an_agent_is_tracked_as_that_agent() {
        let mut t =
            TermState::new(AgentKind::Terminal, TermSize { rows: 6, cols: 40 }, 0, String::new(), Some("ls".into()));
        t.note_input(b"claude\r", 50);
        t.process(b"thinking (esc to interrupt)", 100);
        t.evaluate(200);
        assert_eq!(t.report(None).state, AgentState::Working);
        assert!(t.evaluate(100 + crate::holder::detect::TERMINAL_ACTIVE_MS));
        assert_eq!(t.report(None).state, AgentState::Idle);

        assert!(t.set_nested(nested(AgentKind::Claude, 42, 3_000), 3_000));
        assert_eq!(t.running_kind(), AgentKind::Claude);
        t.process(b"\x1b]0;\xe2\x9c\xb3 Fix login bug\x07working (esc to interrupt)", 3_100);
        t.evaluate(3_200);
        let report = t.report(None);
        assert_eq!(report.state, AgentState::Working);
        assert_eq!(report.activity, "Fix login bug");
        assert_eq!(report.nested, nested(AgentKind::Claude, 42, 3_000));
        assert!(!t.set_nested(nested(AgentKind::Claude, 42, 9_000), 9_000));
        assert_eq!(t.report(None).nested.unwrap().since_ms, 3_000);
        assert_eq!(t.report(None).state, AgentState::Working);

        assert!(t.set_nested(None, 10_000));
        assert_eq!(t.running_kind(), AgentKind::Terminal);
        let report = t.report(None);
        assert_eq!(report.state, AgentState::Idle);
        assert_eq!(report.nested, None);
        assert_eq!(report.activity, "claude");
    }

    #[test]
    fn a_new_program_in_the_terminal_starts_a_fresh_turn() {
        let mut t = state(AgentKind::Terminal);
        t.set_nested(nested(AgentKind::Codex, 7, 100), 100);
        t.process(b"Working (3s \xe2\x80\xa2 esc to interrupt)", 150);
        t.evaluate(200);
        assert_eq!(t.report(None).state, AgentState::Working);
        assert!(t.set_nested(nested(AgentKind::Codex, 8, 300), 300));
        let report = t.report(None);
        assert_eq!(report.state, AgentState::Idle);
        assert_eq!(report.nested.unwrap().since_ms, 300);
        assert_eq!(report.activity, "");
    }

    #[test]
    fn hooks_drive_the_state_of_launched_agents_until_the_user_answers() {
        let mut t = state(AgentKind::Claude);
        assert!(t.apply_hook(HookSignal::Busy, 100));
        assert_eq!(t.report(None).state, AgentState::Working);
        assert!(t.apply_hook(HookSignal::Waiting, 200));
        t.process(b"Bash command\r\nRun it?\r\n> 1. Yes\r\nEnter to confirm \xc2\xb7 Esc to cancel", 250);
        t.evaluate(300);
        assert_eq!(t.report(None).state, AgentState::Paused, "a dialog the screen detector misreads stays paused");
        t.note_input(b"\x1b[B", 350);
        t.evaluate(400);
        assert_eq!(t.report(None).state, AgentState::Paused, "moving through the options is not an answer yet");
        t.note_input(b"\r", 450);
        t.process(b"\x1b[2J\x1b[Hrunning tests (esc to interrupt)", 500);
        t.evaluate(600);
        assert_eq!(t.report(None).state, AgentState::Working);
        assert!(t.apply_hook(HookSignal::Finished, 700));
        let report = t.report(None);
        assert_eq!((report.state, report.state_since_ms, report.last_turn_ms), (AgentState::Done, 700, Some(600)));
    }

    #[test]
    fn hooks_are_ignored_for_terminals_and_programs_started_inside_them() {
        let mut terminal = state(AgentKind::Terminal);
        assert!(!terminal.apply_hook(HookSignal::Busy, 100));
        assert_eq!(terminal.report(None).state, AgentState::Idle);
        terminal.set_nested(nested(AgentKind::Claude, 9, 50), 50);
        assert!(!terminal.apply_hook(HookSignal::Waiting, 100));
        assert_eq!(terminal.report(None).state, AgentState::Idle);
    }

    #[test]
    fn answers_are_enter_escape_or_a_choice_number() {
        assert!(is_answer(b"\r"));
        assert!(is_answer(b"2"));
        assert!(is_answer(b"\x1b"));
        assert!(is_answer(b"\x1b[200~text\n\x1b[201~"));
        assert!(!is_answer(b"\x1b[A"));
        assert!(!is_answer(b"y"));
        assert!(!is_answer(b""));
    }

    #[test]
    fn resize_changes_reported_size() {
        let mut t = state(AgentKind::Terminal);
        t.resize(TermSize { rows: 30, cols: 100 });
        let report = t.report(None);
        assert_eq!((report.rows, report.cols), (30, 100));
    }
}
