use crate::model::{AgentKind, AgentState};

pub const DONE_GRACE_MS: u64 = 2_500;
pub const TERMINAL_ACTIVE_MS: u64 = 1_500;
pub const TAIL_ROWS: usize = 24;

const BUSY_MARKERS: &[&str] = &["esc to interrupt", "esc to cancel", "ctrl+c to interrupt"];

const WAITING_MARKERS: &[&str] = &[
    "do you want to",
    "do you trust",
    "would you like to",
    "allow command",
    "approve this",
    "press enter to continue",
    "press enter to confirm",
    "enter to select",
    "enter to submit answer",
    "enter to submit all",
    "(y/n)",
    "[y/n]",
    "❯ 1. yes",
    "› 1. yes",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    pub busy: bool,
    pub waiting: bool,
}

const SPINNER_GLYPHS: &[char] = &['◐', '◓', '◑', '◒', '◴', '◵', '◶', '◷', '✢', '✶', '✻', '✽', '·'];

pub fn title_is_busy(title: &str) -> bool {
    title
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| (('\u{2801}'..='\u{28FF}').contains(&c)) || SPINNER_GLYPHS.contains(&c))
}

pub fn strip_title_glyphs(title: &str) -> String {
    title
        .trim()
        .trim_start_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '"' | '\'' | '(' | '['))
        .trim()
        .to_string()
}

pub fn analyze(kind: AgentKind, title: &str, tail: &str) -> Signals {
    if !kind.is_ai() {
        return Signals::default();
    }
    let lower = tail.to_lowercase();
    let waiting = WAITING_MARKERS.iter().any(|m| lower.contains(m));
    let busy = !waiting
        && (BUSY_MARKERS.iter().any(|m| lower.contains(m)) || (kind == AgentKind::Claude && title_is_busy(title)));
    Signals { busy, waiting }
}

fn looks_like_program_path(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let is_path = lower.contains('\\') || lower.contains('/');
    let is_program = [".exe", ".cmd", ".bat", ".com"].iter().any(|ext| lower.ends_with(ext));
    is_program || (is_path && !lower.contains(' '))
}

pub fn activity_from_title(kind: AgentKind, title: &str) -> Option<String> {
    let text = strip_title_glyphs(title);
    if text.is_empty() || looks_like_program_path(title) || looks_like_program_path(&text) {
        return None;
    }
    let generic = match kind {
        AgentKind::Claude => text.eq_ignore_ascii_case("claude code") || text.eq_ignore_ascii_case("claude"),
        AgentKind::Codex => text.eq_ignore_ascii_case("codex"),
        AgentKind::Terminal => false,
    };
    (!generic).then_some(text)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detector {
    state: AgentState,
    since: u64,
    turn_started: Option<u64>,
    last_turn_ms: Option<u64>,
    last_signal_at: u64,
}

impl Detector {
    pub fn new(now: u64) -> Self {
        Self { state: AgentState::Idle, since: now, turn_started: None, last_turn_ms: None, last_signal_at: 0 }
    }

    pub fn state(&self) -> AgentState {
        self.state
    }

    pub fn since(&self) -> u64 {
        self.since
    }

    pub fn turn_started(&self) -> Option<u64> {
        self.turn_started
    }

    pub fn last_turn_ms(&self) -> Option<u64> {
        self.last_turn_ms
    }

    pub fn is_busy_or_waiting(&self) -> bool {
        matches!(self.state, AgentState::Working | AgentState::Paused)
    }

    fn set(&mut self, state: AgentState, since: u64) -> bool {
        if self.state == state {
            return false;
        }
        self.state = state;
        self.since = since;
        true
    }

    pub fn observe(&mut self, kind: AgentKind, signals: Signals, last_output_at: u64, now: u64) -> bool {
        if kind.is_ai() { self.observe_ai(signals, now) } else { self.observe_terminal(last_output_at, now) }
    }

    fn observe_ai(&mut self, signals: Signals, now: u64) -> bool {
        if signals.waiting {
            self.last_signal_at = now;
            if !self.is_busy_or_waiting() {
                self.turn_started = Some(now);
            }
            return self.set(AgentState::Paused, now);
        }
        if signals.busy {
            self.last_signal_at = now;
            if !self.is_busy_or_waiting() {
                self.turn_started = Some(now);
            }
            return self.set(AgentState::Working, now);
        }
        if self.is_busy_or_waiting() && now.saturating_sub(self.last_signal_at) >= DONE_GRACE_MS {
            self.last_turn_ms = self.turn_started.map(|start| self.last_signal_at.saturating_sub(start));
            self.turn_started = None;
            return self.set(AgentState::Done, self.last_signal_at);
        }
        false
    }

    fn observe_terminal(&mut self, last_output_at: u64, now: u64) -> bool {
        let active = last_output_at > 0 && now.saturating_sub(last_output_at) < TERMINAL_ACTIVE_MS;
        if active && self.state != AgentState::Working {
            self.turn_started = Some(now);
            return self.set(AgentState::Working, now);
        }
        if !active && self.state == AgentState::Working {
            self.last_turn_ms = self.turn_started.map(|start| last_output_at.saturating_sub(start));
            self.turn_started = None;
            return self.set(AgentState::Idle, last_output_at);
        }
        false
    }

    pub fn mark_seen(&mut self) -> bool {
        if self.state == AgentState::Done {
            self.state = AgentState::Idle;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_WORKING: &str = "✻ Reticulating… (12s · ↑ 1.2k tokens · esc to interrupt)\n> \n";
    const CLAUDE_PERMISSION: &str = " Bash command\n   npm test\n Do you want to proceed?\n ❯ 1. Yes\n   2. No\n";
    const CLAUDE_TRUST: &str = " Do you trust the files in this folder?\n ❯ 1. Yes, proceed\n";
    const CLAUDE_IDLE: &str = "● Done. All tests pass.\n> \n  ? for shortcuts\n";
    const CLAUDE_QUESTION: &str = " ☐ Delete  ☐ Popup  ✔ Submit  →\n Which behavior?\n ❯ 1. Stop and remove\n   2. Same as ctrl+x\n   3. Type something.\n Enter to select · Tab/Arrow keys to navigate · Esc to cancel\n";
    const CODEX_QUESTION: &str =
        " Question 1/2\n  › 1. Keep the API\n    2. Replace it\n  enter to submit answer · esc to interrupt\n";

    #[test]
    fn detects_claude_working_from_screen() {
        let s = analyze(AgentKind::Claude, "✳ Fix tests", CLAUDE_WORKING);
        assert_eq!(s, Signals { busy: true, waiting: false });
    }

    #[test]
    fn detects_claude_working_from_title_spinner() {
        let s = analyze(AgentKind::Claude, "⠐ Fix tests", CLAUDE_IDLE);
        assert!(s.busy);
        assert!(analyze(AgentKind::Claude, "◐ Ok", "✻ Churned for 3s").busy);
        assert!(!analyze(AgentKind::Claude, "✳ Ok", "✻ Churned for 3s · done 7:03 PM").busy);
    }

    #[test]
    fn idle_title_and_screen_is_not_busy() {
        let s = analyze(AgentKind::Claude, "✳ Fix tests", CLAUDE_IDLE);
        assert_eq!(s, Signals::default());
    }

    #[test]
    fn permission_prompt_is_waiting_and_not_busy() {
        let s = analyze(AgentKind::Claude, "⠐ Fix tests", CLAUDE_PERMISSION);
        assert_eq!(s, Signals { busy: false, waiting: true });
        assert!(analyze(AgentKind::Claude, "", CLAUDE_TRUST).waiting);
    }

    #[test]
    fn codex_markers_are_detected() {
        assert!(analyze(AgentKind::Codex, "", "• Working (3s • Esc to interrupt)").busy);
        assert!(analyze(AgentKind::Codex, "", "Would you like to run the following command?").waiting);
    }

    #[test]
    fn questions_waiting_for_a_choice_are_not_reported_as_working() {
        assert_eq!(analyze(AgentKind::Claude, "⠐ Plan", CLAUDE_QUESTION), Signals { busy: false, waiting: true });
        assert_eq!(analyze(AgentKind::Codex, "", CODEX_QUESTION), Signals { busy: false, waiting: true });
        assert!(analyze(AgentKind::Codex, "", "Press enter to confirm or esc to cancel").waiting);
    }

    #[test]
    fn terminals_never_report_ai_signals() {
        assert_eq!(analyze(AgentKind::Terminal, "⠐", CLAUDE_WORKING), Signals::default());
    }

    #[test]
    fn title_glyphs_are_stripped() {
        assert_eq!(strip_title_glyphs("✳ Running design update"), "Running design update");
        assert_eq!(strip_title_glyphs("⠂  Fix bug"), "Fix bug");
        assert_eq!(strip_title_glyphs("plain"), "plain");
        assert_eq!(activity_from_title(AgentKind::Claude, "✳ Claude Code"), None);
        assert_eq!(activity_from_title(AgentKind::Claude, "✳ Shepherd side panel"), Some("Shepherd side panel".into()));
    }

    #[test]
    fn program_path_titles_are_not_activities() {
        assert_eq!(activity_from_title(AgentKind::Claude, r"C:\Users\me\.local\bin\claude.exe"), None);
        assert_eq!(activity_from_title(AgentKind::Terminal, r"C:\WINDOWS\system32\cmd.exe"), None);
        assert_eq!(activity_from_title(AgentKind::Terminal, "/usr/bin/zsh"), None);
        assert_eq!(activity_from_title(AgentKind::Claude, "claude"), None);
        assert_eq!(
            activity_from_title(AgentKind::Terminal, "npm run dev in /srv/app"),
            Some("npm run dev in /srv/app".into())
        );
    }

    #[test]
    fn busy_title_detection_requires_braille_spinner() {
        assert!(title_is_busy("⠋ x"));
        assert!(title_is_busy("◐ Claude Code"));
        assert!(title_is_busy("◑ Ok"));
        assert!(!title_is_busy("✳ Ok"));
        assert!(!title_is_busy("claude"));
        assert!(!title_is_busy(r"C:\Users\me\.local\bin\claude.exe"));
        assert!(!title_is_busy("✳ x"));
        assert!(!title_is_busy(""));
        assert!(!title_is_busy("\u{2800} blank"));
    }

    #[test]
    fn ai_turn_lifecycle_working_done_seen() {
        let busy = Signals { busy: true, waiting: false };
        let mut d = Detector::new(0);
        assert!(d.observe(AgentKind::Claude, busy, 0, 1_000));
        assert_eq!(d.state(), AgentState::Working);
        assert_eq!(d.turn_started(), Some(1_000));
        assert!(!d.observe(AgentKind::Claude, busy, 0, 5_000));
        assert!(!d.observe(AgentKind::Claude, Signals::default(), 0, 6_000));
        assert_eq!(d.state(), AgentState::Working);
        assert!(d.observe(AgentKind::Claude, Signals::default(), 0, 5_000 + DONE_GRACE_MS));
        assert_eq!(d.state(), AgentState::Done);
        assert_eq!(d.since(), 5_000);
        assert_eq!(d.last_turn_ms(), Some(4_000));
        assert!(d.mark_seen());
        assert_eq!(d.state(), AgentState::Idle);
        assert!(!d.mark_seen());
    }

    #[test]
    fn paused_keeps_turn_running() {
        let mut d = Detector::new(0);
        d.observe(AgentKind::Claude, Signals { busy: true, waiting: false }, 0, 100);
        d.observe(AgentKind::Claude, Signals { busy: false, waiting: true }, 0, 200);
        assert_eq!(d.state(), AgentState::Paused);
        assert_eq!(d.turn_started(), Some(100));
        d.observe(AgentKind::Claude, Signals { busy: true, waiting: false }, 0, 300);
        assert_eq!(d.state(), AgentState::Working);
        assert_eq!(d.turn_started(), Some(100));
    }

    #[test]
    fn idle_without_signals_stays_idle() {
        let mut d = Detector::new(0);
        assert!(!d.observe(AgentKind::Claude, Signals::default(), 0, 60_000));
        assert_eq!(d.state(), AgentState::Idle);
    }

    #[test]
    fn terminal_activity_follows_output() {
        let mut d = Detector::new(0);
        assert!(d.observe(AgentKind::Terminal, Signals::default(), 1_000, 1_100));
        assert_eq!(d.state(), AgentState::Working);
        assert!(d.observe(AgentKind::Terminal, Signals::default(), 1_000, 1_000 + TERMINAL_ACTIVE_MS));
        assert_eq!(d.state(), AgentState::Idle);
        assert!(!d.mark_seen());
    }
}
