use ratatui::style::Color;

use crate::model::{AgentKind, AgentState};
use crate::util::fnv1a64;

pub const TEXT: Color = Color::Rgb(222, 222, 228);
pub const MUTED: Color = Color::Rgb(150, 150, 160);
pub const DIM: Color = Color::Rgb(110, 110, 122);
pub const FAINT: Color = Color::Rgb(72, 72, 84);
pub const RULE: Color = Color::Rgb(62, 62, 74);
pub const ACCENT: Color = Color::Rgb(255, 95, 135);
pub const LINK: Color = Color::Rgb(110, 150, 255);
pub const HOST: Color = Color::Rgb(120, 170, 255);
pub const GREEN: Color = Color::Rgb(80, 215, 130);
pub const YELLOW: Color = Color::Rgb(240, 214, 90);
pub const RED: Color = Color::Rgb(240, 78, 92);
pub const INK: Color = Color::Rgb(18, 18, 24);
pub const PAPER: Color = Color::Rgb(236, 236, 240);
pub const CLAUDE: Color = Color::Rgb(222, 128, 92);
pub const CODEX: Color = Color::Rgb(120, 175, 255);
pub const TERMINAL: Color = Color::Rgb(90, 210, 210);
pub const TAB_ACTIVE_BG: Color = Color::Rgb(44, 44, 58);
pub const USAGE_WARN_PERCENT: f64 = 70.0;
pub const USAGE_ALERT_PERCENT: f64 = 90.0;

const PROJECT_PALETTE: [Color; 8] = [
    Color::Rgb(186, 166, 255),
    Color::Rgb(150, 226, 170),
    Color::Rgb(246, 130, 160),
    Color::Rgb(137, 186, 250),
    Color::Rgb(250, 182, 130),
    Color::Rgb(140, 226, 214),
    Color::Rgb(240, 222, 150),
    Color::Rgb(232, 170, 236),
];

pub fn project_color(label: &str) -> Color {
    let index = usize::try_from(fnv1a64(label.as_bytes()) % PROJECT_PALETTE.len() as u64).unwrap_or(0);
    PROJECT_PALETTE[index]
}

pub fn kind_color(kind: AgentKind) -> Color {
    match kind {
        AgentKind::Claude => CLAUDE,
        AgentKind::Codex => CODEX,
        AgentKind::Terminal => TERMINAL,
    }
}

pub fn name_color(kind: AgentKind, state: AgentState) -> Color {
    match state {
        AgentState::Exited | AgentState::Lost => FAINT,
        _ if kind == AgentKind::Terminal => TEXT,
        AgentState::Done => GREEN,
        AgentState::Working => TEXT,
        AgentState::Paused => YELLOW,
        AgentState::Idle => MUTED,
    }
}

pub fn usage_color(used_percent: f64) -> Color {
    if used_percent >= USAGE_ALERT_PERCENT {
        RED
    } else if used_percent >= USAGE_WARN_PERCENT {
        YELLOW
    } else {
        GREEN
    }
}

pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn glyph(kind: AgentKind, state: AgentState, frame: usize) -> (&'static str, Color) {
    match state {
        AgentState::Lost => ("↻", RED),
        AgentState::Exited => ("✗", FAINT),
        _ if kind == AgentKind::Terminal => ("$", TEXT),
        AgentState::Working => (SPINNER[frame % SPINNER.len()], YELLOW),
        AgentState::Paused => ("‖", YELLOW),
        AgentState::Done => ("✔", GREEN),
        AgentState::Idle => ("●", DIM),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_colors_are_stable() {
        assert_eq!(project_color("vibing/earth"), project_color("vibing/earth"));
    }

    #[test]
    fn usage_colors_escalate_near_the_limit() {
        assert_eq!(usage_color(0.0), GREEN);
        assert_eq!(usage_color(69.9), GREEN);
        assert_eq!(usage_color(USAGE_WARN_PERCENT), YELLOW);
        assert_eq!(usage_color(89.0), YELLOW);
        assert_eq!(usage_color(USAGE_ALERT_PERCENT), RED);
        assert_eq!(usage_color(130.0), RED);
    }

    #[test]
    fn glyphs_reflect_state() {
        assert_eq!(glyph(AgentKind::Claude, AgentState::Done, 0).0, "✔");
        assert_eq!(glyph(AgentKind::Terminal, AgentState::Working, 0).0, "$");
        assert_eq!(glyph(AgentKind::Codex, AgentState::Lost, 0).0, "↻");
        assert_eq!(glyph(AgentKind::Claude, AgentState::Paused, 0), ("‖", YELLOW));
        assert_eq!(name_color(AgentKind::Codex, AgentState::Paused), YELLOW);
        assert_ne!(
            glyph(AgentKind::Claude, AgentState::Working, 0).0,
            glyph(AgentKind::Claude, AgentState::Working, 1).0
        );
    }
}
