use crate::hooks::HookSignal;
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
    "enter to confirm",
    "enter to select",
    "enter to submit answer",
    "enter to submit all",
    "(y/n)",
    "[y/n]",
    "❯ 1. yes",
    "› 1. yes",
];

const CLAUDE_WAITING_MARKERS: &[&str] = &["running in bypass permissions mode"];
const OPENCODE_BUSY_MARKERS: &[&str] = &["esc interrupt", "esc again to interrupt"];
const OPENCODE_WAITING_MARKERS: &[&str] = &["permission required", "type your own answer"];
const ANTIGRAVITY_WAITING_MARKERS: &[&str] = &[
    "run this command?",
    "allow creation of this file?",
    "accept this file edit",
    "allow access to this",
    "allow calling this tool?",
    "trust this folder?",
];
const GROK_BUSY_MARKERS: &[&str] = &["[stop]"];
const GROK_WAITING_MARKERS: &[&str] = &["tell grok what to do differently", "waiting on plan approval"];
const QWEN_WAITING_MARKERS: &[&str] = &["waiting for user confirmation", "yes, allow once"];
const KIMI_BUSY_MARKERS: &[&str] = &["working…", "🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"];
const KIMI_WAITING_MARKERS: &[&str] = &["approve once", "trust this folder?", "ready to submit your answers?"];
const QWEN_BUSY_GLYPH: char = '◐';
const QWEN_WAITING_GLYPH: char = '✳';
const GROK_ATTENTION: &str = "Action Required";
const GROK_STATES: &[&str] = &["Thinking", "Responding", "Compacting", "Waiting", "Cancelling"];
const GROK_RUNNING: &str = "Running:";
const GROK_NAME: &str = "grok";
const TITLE_SEPARATOR: &str = " - ";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    pub busy: bool,
    pub waiting: bool,
}

const SPINNER_GLYPHS: &[char] = &['◐', '◓', '◑', '◒', '◴', '◵', '◶', '◷', '✢', '✶', '✻', '✽', '·'];

fn is_braille_spinner(c: char) -> bool {
    ('\u{2801}'..='\u{28FF}').contains(&c)
}

pub fn title_is_busy(title: &str) -> bool {
    title.trim_start().chars().next().is_some_and(|c| is_braille_spinner(c) || SPINNER_GLYPHS.contains(&c))
}

fn busy_markers(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::Kilo | AgentKind::Opencode => OPENCODE_BUSY_MARKERS,
        AgentKind::Grok => GROK_BUSY_MARKERS,
        AgentKind::Kimi => KIMI_BUSY_MARKERS,
        _ => &[],
    }
}

fn waiting_markers(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::Claude => CLAUDE_WAITING_MARKERS,
        AgentKind::Kilo | AgentKind::Opencode => OPENCODE_WAITING_MARKERS,
        AgentKind::Antigravity => ANTIGRAVITY_WAITING_MARKERS,
        AgentKind::Grok => GROK_WAITING_MARKERS,
        AgentKind::Qwen => QWEN_WAITING_MARKERS,
        AgentKind::Kimi => KIMI_WAITING_MARKERS,
        _ => &[],
    }
}

fn title_signals(kind: AgentKind, title: &str) -> Signals {
    let lead = title.trim_start().chars().next();
    match kind {
        AgentKind::Claude => Signals { busy: title_is_busy(title), waiting: false },
        AgentKind::Qwen => Signals { busy: lead == Some(QWEN_BUSY_GLYPH), waiting: lead == Some(QWEN_WAITING_GLYPH) },
        AgentKind::Grok => {
            Signals { busy: lead.is_some_and(is_braille_spinner), waiting: title.contains(GROK_ATTENTION) }
        }
        _ => Signals::default(),
    }
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
    let title = title_signals(kind, title);
    let waiting = title.waiting || WAITING_MARKERS.iter().chain(waiting_markers(kind)).any(|m| lower.contains(m));
    let busy = !waiting && (title.busy || BUSY_MARKERS.iter().chain(busy_markers(kind)).any(|m| lower.contains(m)));
    Signals { busy, waiting }
}

fn looks_like_program_path(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    let is_path = lower.contains('\\') || lower.contains('/');
    let is_program = [".exe", ".cmd", ".bat", ".com"].iter().any(|ext| lower.ends_with(ext));
    is_program || (is_path && !lower.contains(' '))
}

fn generic_titles(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::Claude => &["claude code", "claude"],
        AgentKind::Codex => &["codex"],
        AgentKind::Kilo => &["kilo cli", "kilo"],
        AgentKind::Opencode => &["opencode"],
        AgentKind::Antigravity => &["antigravity cli", "antigravity", "agy"],
        AgentKind::Grok => &[GROK_NAME],
        AgentKind::Qwen => &["qwen"],
        AgentKind::Kimi => &["kimi code", "kimi"],
        AgentKind::Terminal => &[],
    }
}

fn title_prefix(kind: AgentKind) -> Option<&'static str> {
    match kind {
        AgentKind::Kilo => Some("Kilo CLI | "),
        AgentKind::Opencode => Some("OC | "),
        _ => None,
    }
}

fn generic_prefix(kind: AgentKind) -> Option<&'static str> {
    match kind {
        AgentKind::Qwen => Some("Qwen - "),
        _ => None,
    }
}

fn grok_title(text: &str) -> String {
    let parts: Vec<&str> = text
        .split(TITLE_SEPARATOR)
        .map(str::trim)
        .filter(|part| !strip_title_glyphs(part).is_empty())
        .filter(|part| !part.contains(GROK_ATTENTION) && !part.starts_with(GROK_RUNNING))
        .filter(|part| !GROK_STATES.iter().any(|state| part.trim_end_matches('…').eq_ignore_ascii_case(state)))
        .collect();
    let parts = match parts.split_last() {
        Some((last, rest)) if last.eq_ignore_ascii_case(GROK_NAME) => rest,
        _ => &parts[..],
    };
    parts.join(TITLE_SEPARATOR)
}

pub fn activity_from_title(kind: AgentKind, title: &str) -> Option<String> {
    let text = strip_title_glyphs(title);
    if text.is_empty() || looks_like_program_path(title) || looks_like_program_path(&text) {
        return None;
    }
    if generic_prefix(kind).is_some_and(|prefix| text.starts_with(prefix)) {
        return None;
    }
    let text = match kind {
        AgentKind::Grok => grok_title(&text),
        _ => title_prefix(kind).and_then(|prefix| text.strip_prefix(prefix)).unwrap_or(&text).trim().to_string(),
    };
    let generic = text.is_empty() || generic_titles(kind).iter().any(|name| text.eq_ignore_ascii_case(name));
    (!generic).then_some(text)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lease {
    Waiting,
    Finished { screen_settled: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detector {
    state: AgentState,
    since: u64,
    turn_started: Option<u64>,
    last_turn_ms: Option<u64>,
    last_signal_at: u64,
    lease: Option<Lease>,
}

impl Detector {
    pub fn new(now: u64) -> Self {
        Self {
            state: AgentState::Idle,
            since: now,
            turn_started: None,
            last_turn_ms: None,
            last_signal_at: 0,
            lease: None,
        }
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

    fn begin_turn(&mut self, at: u64) {
        if !self.is_busy_or_waiting() {
            self.turn_started = Some(at);
        }
        self.last_signal_at = self.last_signal_at.max(at);
    }

    pub fn hook(&mut self, signal: HookSignal, at: u64) -> bool {
        match signal {
            HookSignal::Busy => {
                self.lease = None;
                self.begin_turn(at);
                self.set(AgentState::Working, at)
            }
            HookSignal::Waiting => {
                self.lease = Some(Lease::Waiting);
                self.begin_turn(at);
                self.set(AgentState::Paused, at)
            }
            HookSignal::Finished => {
                self.lease = Some(Lease::Finished { screen_settled: false });
                if !self.is_busy_or_waiting() {
                    return false;
                }
                self.last_turn_ms = self.turn_started.map(|start| at.saturating_sub(start));
                self.turn_started = None;
                self.set(AgentState::Done, at)
            }
        }
    }

    pub fn user_answered(&mut self) {
        if self.lease == Some(Lease::Waiting) {
            self.lease = None;
        }
    }

    fn observe_ai(&mut self, signals: Signals, now: u64) -> bool {
        match self.lease {
            Some(Lease::Waiting) => {
                self.last_signal_at = now;
                return false;
            }
            Some(Lease::Finished { screen_settled }) => {
                if !signals.busy {
                    self.lease = Some(Lease::Finished { screen_settled: true });
                    return false;
                }
                if !screen_settled {
                    return false;
                }
                self.lease = None;
            }
            None => {}
        }
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
    fn the_folder_trust_check_waits_for_you_instead_of_looking_busy() {
        let trust = " Quick safety check: Is this a project you created or one you trust?\n ❯ No, exit\n   Yes, I trust this folder\n\n Enter to confirm · Esc to cancel\n";
        assert_eq!(analyze(AgentKind::Claude, "", trust), Signals { busy: false, waiting: true });
    }

    #[test]
    fn the_bypass_mode_warning_waits_for_you_until_it_is_accepted() {
        let warning = " WARNING: Claude Code running in Bypass Permissions mode\n\n In Bypass Permissions mode, Claude Code will not ask for your approval before running potentially dangerous commands.\n\n ❯ 1. No, exit\n   2. Yes, I accept\n";
        assert_eq!(analyze(AgentKind::Claude, "", warning), Signals { busy: false, waiting: true });
        let accepted = "> \n  ⏵⏵ bypass permissions on (shift+tab to cycle)\n";
        assert_eq!(analyze(AgentKind::Claude, "", accepted), Signals::default());
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
    fn opencode_and_kilo_screens_show_work_permissions_and_questions() {
        for kind in [AgentKind::Opencode, AgentKind::Kilo] {
            let working = "  ■■⬝⬝⬝⬝⬝⬝  esc interrupt\n";
            assert_eq!(analyze(kind, "OpenCode", working), Signals { busy: true, waiting: false });
            assert!(analyze(kind, "", "esc again to interrupt").busy);
            let permission =
                " △ Permission required\n   $ npm test\n  Allow once   Allow always   Reject\n  esc interrupt";
            assert_eq!(analyze(kind, "", permission), Signals { busy: false, waiting: true });
            assert!(analyze(kind, "", " 1. Keep it\n 2. Type your own answer\n").waiting);
            let finished = "▣  Build · anthropic/claude-sonnet · 12.3s\n  tab agents  ctrl+p commands";
            assert_eq!(analyze(kind, "OC | Fix login", finished), Signals::default());
        }
        assert!(!analyze(AgentKind::Claude, "", " △ Permission required").waiting, "markers are per agent");
    }

    #[test]
    fn antigravity_approval_screens_wait_for_you() {
        assert!(analyze(AgentKind::Antigravity, "", " Run this command?\n Yes, run command\n").waiting);
        assert!(analyze(AgentKind::Antigravity, "", " Allow calling this tool?\n").waiting);
        assert!(!analyze(AgentKind::Antigravity, "", " The user has a question about it.\n").waiting);
        assert!(analyze(AgentKind::Antigravity, "", "Working… (esc to cancel)").busy);
        assert_eq!(analyze(AgentKind::Antigravity, "", "> "), Signals::default());
    }

    #[test]
    fn grok_title_and_screen_show_work_and_approvals() {
        assert!(analyze(AgentKind::Grok, "⠋ - Thinking - fix login - grok", "").busy);
        assert!(analyze(AgentKind::Grok, "fix login - grok", "⠧ Running… 12s   1m02s ⇣12k [stop]").busy);
        let approval = "⚠ Action Required - ⠋ - Waiting - fix login - grok";
        assert_eq!(analyze(AgentKind::Grok, approval, ""), Signals { busy: false, waiting: true });
        let prompt = " Allow `npm`?\n 1. Yes, proceed\n 2. No, and tell Grok what to do differently\n";
        assert_eq!(analyze(AgentKind::Grok, "fix login - grok", prompt), Signals { busy: false, waiting: true });
        assert_eq!(analyze(AgentKind::Grok, "fix login - grok", "Worked for 2m5s\n> "), Signals::default());
        assert!(!analyze(AgentKind::Claude, "⚠ Action Required - grok", "").waiting);
    }

    #[test]
    fn qwen_title_glyphs_tell_working_from_waiting() {
        let busy = format!("{QWEN_BUSY_GLYPH}\u{FE0E} Fix login{}", " ".repeat(40));
        assert_eq!(analyze(AgentKind::Qwen, &busy, ""), Signals { busy: true, waiting: false });
        let waiting = format!("{QWEN_WAITING_GLYPH}\u{FE0E} Fix login");
        assert_eq!(analyze(AgentKind::Qwen, &waiting, ""), Signals { busy: false, waiting: true });
        assert!(analyze(AgentKind::Qwen, "Qwen - app", "(5s · ↓ 1.2k tokens · esc to cancel)").busy);
        assert!(analyze(AgentKind::Qwen, "", "Allow execution of: 'rm -rf build'?\n ● 1. Yes, allow once").waiting);
        assert_eq!(analyze(AgentKind::Qwen, "Qwen - app", "> "), Signals::default());
        assert!(!analyze(AgentKind::Claude, &waiting, "").waiting, "the idle claude title starts with the same glyph");
    }

    #[test]
    fn kimi_spinners_and_approval_panel_are_detected() {
        assert!(analyze(AgentKind::Kimi, "Kimi Code", "⠋ Working… · Tip: use /usage").busy);
        assert!(analyze(AgentKind::Kimi, "Kimi Code", "🌓").busy);
        let approval = "───\n ▶ Run this command?\n ▶ 1. Approve once\n   2. Approve for this session\n   3. Reject";
        assert_eq!(analyze(AgentKind::Kimi, "", approval), Signals { busy: false, waiting: true });
        assert!(analyze(AgentKind::Kimi, "", " Trust this folder?\n Trust and continue").waiting);
        assert_eq!(analyze(AgentKind::Kimi, "Kimi Code", "Interrupted by user\n> "), Signals::default());
    }

    #[test]
    fn titles_of_the_new_agents_give_their_session_names_only() {
        assert_eq!(activity_from_title(AgentKind::Opencode, "OpenCode"), None);
        assert_eq!(activity_from_title(AgentKind::Opencode, "OC | Fix login flow"), Some("Fix login flow".into()));
        assert_eq!(activity_from_title(AgentKind::Kilo, "Kilo CLI"), None);
        assert_eq!(activity_from_title(AgentKind::Kilo, "◔ Kilo CLI | Fix login"), Some("Fix login".into()));
        assert_eq!(activity_from_title(AgentKind::Antigravity, "Antigravity CLI"), None);
        assert_eq!(activity_from_title(AgentKind::Kimi, "Kimi Code"), None);
        assert_eq!(activity_from_title(AgentKind::Kimi, "Refactor the parser"), Some("Refactor the parser".into()));
        assert_eq!(activity_from_title(AgentKind::Qwen, "Qwen - orkai_cli          "), None);
        assert_eq!(activity_from_title(AgentKind::Qwen, "◐\u{FE0E} Fix login   "), Some("Fix login".into()));
        assert_eq!(activity_from_title(AgentKind::Grok, "grok"), None);
        assert_eq!(activity_from_title(AgentKind::Grok, "fix login - grok"), Some("fix login".into()));
        assert_eq!(activity_from_title(AgentKind::Grok, "⠋ - Thinking - fix login - grok"), Some("fix login".into()));
        assert_eq!(
            activity_from_title(AgentKind::Grok, "⚠ Action Required - ⠙ - Running: bash - a - b - grok"),
            Some("a - b".into())
        );
        assert_eq!(activity_from_title(AgentKind::Grok, "⠋ - Responding… - grok"), None);
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

    fn hook_turn() -> Detector {
        let mut d = Detector::new(0);
        assert!(d.hook(HookSignal::Busy, 1_000));
        assert_eq!(d.state(), AgentState::Working);
        assert_eq!(d.turn_started(), Some(1_000));
        d
    }

    #[test]
    fn a_finished_hook_ends_the_turn_at_once_and_question_text_cannot_reopen_it() {
        let mut d = hook_turn();
        let busy = Signals { busy: true, waiting: false };
        let question = Signals { busy: false, waiting: true };
        d.observe(AgentKind::Claude, busy, 0, 2_000);
        assert!(d.hook(HookSignal::Finished, 3_000));
        assert_eq!((d.state(), d.since(), d.last_turn_ms()), (AgentState::Done, 3_000, Some(2_000)));
        assert!(!d.observe(AgentKind::Claude, busy, 0, 3_050), "the spinner still on screen is not a new turn");
        assert!(!d.observe(AgentKind::Claude, question, 0, 3_200));
        assert!(!d.observe(AgentKind::Claude, Signals::default(), 0, 60_000));
        assert_eq!(d.state(), AgentState::Done);
        assert!(d.mark_seen());
        assert!(d.observe(AgentKind::Claude, busy, 0, 61_000), "a turn started without a prompt is still seen");
        assert_eq!((d.state(), d.turn_started()), (AgentState::Working, Some(61_000)));
    }

    #[test]
    fn a_waiting_hook_holds_until_the_user_answers() {
        let mut d = hook_turn();
        let busy = Signals { busy: true, waiting: false };
        assert!(d.hook(HookSignal::Waiting, 2_000));
        assert_eq!((d.state(), d.turn_started()), (AgentState::Paused, Some(1_000)));
        assert!(!d.observe(AgentKind::Claude, busy, 0, 2_100), "a dialog footer like esc to cancel is not work");
        assert!(!d.observe(AgentKind::Claude, Signals::default(), 0, 2_000 + 10 * DONE_GRACE_MS));
        assert_eq!(d.state(), AgentState::Paused);
        d.user_answered();
        assert!(d.observe(AgentKind::Claude, busy, 0, 40_000));
        assert_eq!((d.state(), d.turn_started()), (AgentState::Working, Some(1_000)));
    }

    #[test]
    fn after_an_answer_without_work_the_screen_decides_again() {
        let mut d = hook_turn();
        d.hook(HookSignal::Waiting, 2_000);
        d.observe(AgentKind::Claude, Signals::default(), 0, 9_000);
        d.user_answered();
        assert!(!d.observe(AgentKind::Claude, Signals::default(), 0, 9_000 + DONE_GRACE_MS - 1));
        assert!(d.observe(AgentKind::Claude, Signals::default(), 0, 9_000 + DONE_GRACE_MS));
        assert_eq!(d.state(), AgentState::Done);
    }

    #[test]
    fn busy_hooks_clear_any_hold_and_leave_the_screen_in_charge() {
        let mut d = hook_turn();
        d.hook(HookSignal::Waiting, 2_000);
        assert!(d.hook(HookSignal::Busy, 3_000));
        assert_eq!((d.state(), d.turn_started()), (AgentState::Working, Some(1_000)));
        assert!(d.observe(AgentKind::Claude, Signals { busy: false, waiting: true }, 0, 3_100));
        assert_eq!(d.state(), AgentState::Paused);
        d.hook(HookSignal::Finished, 4_000);
        assert!(d.hook(HookSignal::Busy, 5_000));
        assert_eq!((d.state(), d.turn_started()), (AgentState::Working, Some(5_000)));
    }

    #[test]
    fn a_late_finished_hook_does_not_bring_back_a_turn_already_seen() {
        let mut d = hook_turn();
        d.observe(AgentKind::Claude, Signals::default(), 0, 1_000 + DONE_GRACE_MS);
        assert_eq!(d.state(), AgentState::Done);
        assert!(d.mark_seen());
        assert!(!d.hook(HookSignal::Finished, 5_000));
        assert_eq!(d.state(), AgentState::Idle);
    }

    #[test]
    fn answering_outside_a_waiting_hold_changes_nothing() {
        let mut d = Detector::new(0);
        d.user_answered();
        assert_eq!(d, Detector::new(0));
    }
}
