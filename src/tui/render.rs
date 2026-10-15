use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget, Wrap};

use crate::model::{AgentId, AgentKind, AgentState};
use crate::tui::app::{AgentEntry, App, Confirm, Hit, Link, Prompt, Tone, View};
use crate::tui::complete::matching_commands;
use crate::tui::format::{duration_short, pad, pad_left, truncate, truncate_start, width};
use crate::tui::sprite;
use crate::tui::theme;
use crate::usage::{self, Problem, Window};
use crate::util::now_ms;

pub const SIDEBAR_WIDTH: u16 = 24;
pub const SIDEBAR_MIN_SCREEN: u16 = 72;
const MAIN_GAP: u16 = 2;
const NAME_MIN: usize = 14;
const NAME_MAX: usize = 30;
const TIME_WIDTH: usize = 6;
const KIND_WIDTH: usize = 9;
const SUBAGENT_LABEL: &str = "subagent";
const SUBAGENT_INDENT: u16 = 6;
const TAB_NAME_MAX: usize = 18;
pub const MAX_COMPOSER_ROWS: u16 = 6;
const HINT: &str = "↑↓ select · enter open · tab who · ^l where · ^n session · ^e rename · del delete · f1 help";
const CONFIRM_WIDTH: u16 = 64;
const CONFIRM_HEIGHT: u16 = 8;
const CONFIRM_HINT: &str = "y / enter confirm  ·  any other key cancels";
const USAGE_GAP: u16 = 4;
const USAGE_NAME_WIDTH: u16 = 8;
const USAGE_WINDOW_WIDTH: usize = 6;
const USAGE_BAR_WIDTH: usize = 12;
const USAGE_PERCENT_WIDTH: usize = 4;
const USAGE_SESSION_LABEL: &str = "5h";
const USAGE_WEEKLY_LABEL: &str = "week";

pub struct Areas {
    pub sidebar: Option<Rect>,
    pub divider: Option<Rect>,
    pub tabs: Option<Rect>,
    pub body: Rect,
    pub rule: Rect,
    pub bottom: Rect,
}

pub fn areas(screen: Rect, agent_view: bool, has_tabs: bool, input_rows: u16) -> Areas {
    let bottom_height = (if agent_view { 1 } else { 2 + input_rows.clamp(1, MAX_COMPOSER_ROWS) }).min(screen.height);
    let rule_height = u16::from(screen.height > bottom_height);
    let top_height = screen.height.saturating_sub(bottom_height + rule_height);
    let top = Rect::new(screen.x, screen.y, screen.width, top_height);
    let rule = Rect::new(screen.x, screen.y + top_height, screen.width, rule_height);
    let bottom = Rect::new(screen.x, rule.y + rule_height, screen.width, bottom_height);
    let (sidebar, divider, main) = if screen.width >= SIDEBAR_MIN_SCREEN {
        let sidebar = Rect::new(top.x, top.y, SIDEBAR_WIDTH, top.height);
        let divider = Rect::new(top.x + SIDEBAR_WIDTH, top.y, 1, top.height);
        let main_x = divider.x + MAIN_GAP;
        let main = Rect::new(main_x, top.y, screen.right().saturating_sub(main_x), top.height);
        (Some(sidebar), Some(divider), main)
    } else {
        (None, None, Rect::new(top.x + 1, top.y, top.width.saturating_sub(1), top.height))
    };
    let tabs = (has_tabs && main.height > 1).then(|| Rect::new(main.x, main.y, main.width, 1));
    let body = match tabs {
        Some(_) => Rect::new(main.x, main.y + 1, main.width, main.height - 1),
        None => main,
    };
    Areas { sidebar, divider, tabs, body, rule, bottom }
}

fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style, limit: u16) -> u16 {
    if x >= limit {
        return x;
    }
    let (end, _) = buf.set_stringn(x, y, text, usize::from(limit - x), style);
    end
}

fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.hits.clear();
    let screen = frame.area();
    let agent_view = matches!(app.view, View::Agent(_));
    let layout = areas(screen, agent_view, agent_view || !app.tabs.is_empty(), composer_rows(app));
    let cursor = {
        let buf = frame.buffer_mut();
        if let Some(sidebar) = layout.sidebar {
            draw_sidebar(app, sidebar, buf);
        }
        if let Some(divider) = layout.divider {
            for y in divider.top()..divider.bottom() {
                put(buf, divider.x, y, "│", fg(theme::RULE), divider.right());
            }
        }
        if layout.rule.height > 0 {
            let line = "─".repeat(usize::from(layout.rule.width));
            put(buf, layout.rule.x, layout.rule.y, &line, fg(theme::RULE), layout.rule.right());
            if let Some(divider) = layout.divider {
                put(buf, divider.x, layout.rule.y, "┴", fg(theme::RULE), divider.right());
            }
        }
        if let Some(tabs) = layout.tabs {
            draw_tabs(app, tabs, buf);
        }
        let cursor = match app.view {
            View::Overview => {
                draw_overview(app, layout.body, buf);
                draw_composer(app, layout.bottom, buf)
            }
            View::Agent(id) => {
                let cursor = draw_agent(app, id, layout.body, buf);
                draw_agent_status(app, id, layout.bottom, buf);
                cursor
            }
        };
        if app.show_help {
            draw_help(screen, buf);
            None
        } else if let Some(Prompt::Confirm(confirm)) = app.prompt.as_ref()
            && confirm.is_popup()
        {
            draw_confirm(app, confirm, screen, buf);
            None
        } else {
            cursor
        }
    };
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

fn draw_sidebar(app: &mut App, area: Rect, buf: &mut Buffer) {
    let x = area.x + 1;
    let right = area.right().saturating_sub(1);
    let mut y = area.y;
    if area.height == 0 {
        return;
    }
    put(buf, x, y, &app.hostname, fg(theme::TEXT).add_modifier(Modifier::BOLD), right);
    y += 1;
    let sessions = app.sessions.clone();
    for session in &sessions {
        if y >= area.bottom() {
            return;
        }
        let selected = session.name == app.session;
        let count = app.session_count(&session.name).to_string();
        let (marker, style) =
            if selected { ("› ", fg(theme::ACCENT).add_modifier(Modifier::BOLD)) } else { ("  ", fg(theme::TEXT)) };
        let count_x = right.saturating_sub(u16::try_from(count.len()).unwrap_or(0));
        put(buf, x, y, marker, style, right);
        put(buf, x + 2, y, &truncate(&session.name, usize::from(count_x.saturating_sub(x + 3))), style, count_x);
        put(buf, count_x, y, &count, if selected { style } else { fg(theme::MUTED) }, right + 1);
        app.hits.push((Rect::new(area.x, y, area.width, 1), Hit::Session(session.name.clone())));
        y += 1;
    }
    if y < area.bottom() {
        put(buf, x + 2, y, "+ new session", fg(theme::LINK), right);
        app.hits.push((Rect::new(area.x, y, area.width, 1), Hit::NewSession));
        y += 2;
    }
    let lost = app.lost_in_session().len();
    if lost > 0 && y + 1 < area.bottom() {
        put(buf, x, y, &format!("↻ {lost} interrupted"), fg(theme::RED).add_modifier(Modifier::BOLD), right);
        put(buf, x, y + 1, "  ctrl+r to recover", fg(theme::MUTED), right);
        app.hits.push((Rect::new(area.x, y, area.width, 2), Hit::Recover));
    }
    if app.launching > 0 && area.height > 2 {
        let text = format!("starting {}…", app.launching);
        put(buf, x, area.bottom() - 2, &text, fg(theme::YELLOW), right);
    }
    if area.height > 1 {
        put(buf, x, area.bottom() - 1, "f1 help", fg(theme::FAINT), right);
    }
}

fn tab_label(entry: &AgentEntry, tick: usize) -> (String, Color) {
    let (glyph, color) = theme::glyph(entry.record.kind, entry.state(), tick);
    (format!("{glyph} {}", truncate(&entry.record.name, TAB_NAME_MAX)), color)
}

fn draw_tabs(app: &mut App, area: Rect, buf: &mut Buffer) {
    let right = area.right();
    let mut x = area.x;
    let overview_active = app.view == View::Overview;
    let style = if overview_active {
        fg(theme::TEXT).bg(theme::TAB_ACTIVE_BG).add_modifier(Modifier::BOLD)
    } else {
        fg(theme::MUTED)
    };
    let start = x;
    x = put(buf, x, area.y, " ◆ overview ", style, right);
    app.hits.push((Rect::new(start, area.y, x - start, 1), Hit::Overview));
    let tabs = app.tabs.clone();
    for (index, id) in tabs.iter().enumerate() {
        let Some(entry) = app.agents.get(id) else { continue };
        if x + 4 >= right {
            break;
        }
        x = put(buf, x, area.y, "│", fg(theme::RULE), right);
        let active = app.view == View::Agent(*id);
        let (label, glyph_color) = tab_label(entry, app.tick);
        let base = if active {
            Style::default().bg(theme::TAB_ACTIVE_BG).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let start = x;
        x = put(buf, x, area.y, " ", base, right);
        let mut chars = label.chars();
        let glyph: String = chars.next().map(String::from).unwrap_or_default();
        x = put(buf, x, area.y, &glyph, base.fg(glyph_color), right);
        let rest: String = chars.collect();
        let name_color = if active { theme::TEXT } else { theme::MUTED };
        x = put(buf, x, area.y, &rest, base.fg(name_color), right);
        let hint = if index < 9 { format!(" {}", index + 1) } else { String::new() };
        x = put(buf, x, area.y, &hint, base.fg(theme::FAINT), right);
        app.hits.push((Rect::new(start, area.y, x.saturating_sub(start), 1), Hit::Tab(*id)));
        let close_start = x;
        x = put(buf, x, area.y, " × ", base.fg(theme::DIM), right);
        app.hits.push((Rect::new(close_start, area.y, x.saturating_sub(close_start), 1), Hit::CloseTab(*id)));
    }
}

fn draw_overview(app: &mut App, area: Rect, buf: &mut Buffer) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let header_height = sprite::HEIGHT.min(area.height);
    sprite::render(
        Rect::new(area.x, area.y, sprite::WIDTH.min(area.width), header_height),
        buf,
        app.any_working(),
        app.tick / 3,
    );
    let stats = app.stats();
    let stats_x = area.x + sprite::WIDTH + 2;
    let right = area.right();
    let rows = [
        (stats.agents, "agents", theme::PAPER, theme::INK),
        (stats.working, "working", theme::YELLOW, theme::INK),
        (stats.paused, "paused", theme::RED, theme::PAPER),
        (stats.done, "done", theme::GREEN, theme::INK),
    ];
    let first = area.y + header_height.saturating_sub(4);
    let mut stats_right = stats_x;
    for (i, (value, label, bg, ink)) in rows.iter().enumerate() {
        let y = first + u16::try_from(i).unwrap_or(0);
        if y < area.bottom() {
            let boxed = format!("{value:>3} ");
            let box_style = Style::default().fg(*ink).bg(*bg).add_modifier(Modifier::BOLD);
            let after = put(buf, stats_x, y, &boxed, box_style, right);
            stats_right = stats_right.max(put(buf, after + 1, y, label, fg(theme::MUTED), right));
        }
    }
    let usage_area = Rect::new(area.x, area.y, area.width, header_height);
    draw_usage(app, stats_right + USAGE_GAP, first, usage_area, buf);
    let list_top = area.y + header_height + 1;
    if list_top < area.bottom() {
        draw_agent_list(app, Rect::new(area.x, list_top, area.width, area.bottom() - list_top), buf);
    }
}

fn usage_cells(used_percent: f64, cells: usize) -> usize {
    let share = used_percent.clamp(0.0, 100.0) / 100.0;
    ((share * cells as f64).round() as usize).min(cells)
}

fn draw_usage_window(window: &Window, now: u64, x: u16, y: u16, right: u16, buf: &mut Buffer) -> u16 {
    let color = theme::usage_color(window.used_percent);
    let filled = usage_cells(window.used_percent, USAGE_BAR_WIDTH);
    let mut x = put(buf, x, y, &"━".repeat(filled), fg(color), right);
    x = put(buf, x, y, &"─".repeat(USAGE_BAR_WIDTH - filled), fg(theme::RULE), right);
    let percent = pad_left(&format!("{:.0}%", window.used_percent), USAGE_PERCENT_WIDTH);
    x = put(buf, x + 1, y, &percent, fg(color).add_modifier(Modifier::BOLD), right);
    if let Some(resets_at) = window.resets_at_ms {
        let text = if resets_at > now {
            format!("resets {}", duration_short(resets_at - now))
        } else {
            "resets now".to_string()
        };
        x = put(buf, x + 2, y, &text, fg(theme::DIM), right);
    }
    x
}

fn draw_usage(app: &App, x: u16, first: u16, area: Rect, buf: &mut Buffer) {
    let right = area.right();
    if x >= right || first >= area.bottom() {
        return;
    }
    let now = now_ms();
    if first > area.y {
        let updated = app.usage.values().filter_map(|u| u.updated_at_ms).max();
        let status = match updated {
            Some(at) => format!(" · updated {} ago", duration_short(now.saturating_sub(at))),
            None => " · checking…".to_string(),
        };
        let after = put(buf, x, first - 1, "usage", fg(theme::MUTED), right);
        put(buf, after, first - 1, &status, fg(theme::FAINT), right);
    }
    for (index, kind) in usage::PROVIDERS.iter().enumerate() {
        let y = first + 2 * u16::try_from(index).unwrap_or(0);
        if y >= area.bottom() {
            return;
        }
        let entry = app.usage.get(kind);
        put(buf, x, y, kind.label(), fg(theme::kind_color(*kind)), right);
        let window_x = x + USAGE_NAME_WIDTH;
        let Some(limits) = entry.and_then(|u| u.limits) else {
            let text = entry.and_then(|u| u.problem).map_or("checking…", Problem::label);
            put(buf, window_x, y, text, fg(theme::FAINT), right);
            continue;
        };
        let windows = [(USAGE_SESSION_LABEL, limits.session), (USAGE_WEEKLY_LABEL, limits.weekly)];
        for (row, (label, window)) in windows.iter().enumerate() {
            let y = y + u16::try_from(row).unwrap_or(0);
            if y >= area.bottom() {
                return;
            }
            let cx = put(buf, window_x, y, &pad(label, USAGE_WINDOW_WIDTH), fg(theme::DIM), right);
            let end = match window {
                Some(window) => draw_usage_window(window, now, cx, y, right, buf),
                None => put(buf, cx, y, "n/a", fg(theme::FAINT), right),
            };
            if row == 0
                && let Some(problem) = entry.and_then(|u| u.problem)
            {
                put(buf, end, y, &format!(" · {}", problem.label()), fg(theme::FAINT), right);
            }
        }
    }
}

enum ListLine {
    Header(String, std::path::PathBuf, usize),
    Agent(AgentId),
    Subagent(AgentId, usize),
}

fn time_columns(entry: &AgentEntry, now: u64) -> (Option<(String, Color)>, Option<String>) {
    let Some(status) = entry.status.as_ref().filter(|_| !entry.is_dead()) else {
        return (None, None);
    };
    let uptime = Some(duration_short(now.saturating_sub(status.started_at_ms)));
    if entry.record.kind == AgentKind::Terminal {
        return (None, uptime);
    }
    let first = match status.state {
        AgentState::Working => {
            let start = status.turn_started_ms.unwrap_or(status.state_since_ms);
            Some((duration_short(now.saturating_sub(start)), theme::YELLOW))
        }
        AgentState::Paused => Some((duration_short(now.saturating_sub(status.state_since_ms)), theme::YELLOW)),
        AgentState::Done | AgentState::Idle if status.last_turn_ms.is_some() => {
            Some((duration_short(now.saturating_sub(status.state_since_ms)), theme::DIM))
        }
        _ => None,
    };
    (first, uptime)
}

fn activity(entry: &AgentEntry) -> (String, Color) {
    if entry.is_starting() {
        return ("starting…".to_string(), theme::DIM);
    }
    match &entry.link {
        Link::Lost => ("interrupted — ctrl+r to recover".to_string(), theme::RED),
        Link::Exited(info) => (info.describe(), theme::FAINT),
        _ => {
            let status = entry.status.as_ref();
            let text = status
                .map(|s| s.activity.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| entry.record.task.split_whitespace().collect::<Vec<_>>().join(" "));
            let color = match entry.state() {
                AgentState::Working => theme::TEXT,
                AgentState::Paused => theme::YELLOW,
                _ => theme::MUTED,
            };
            let text = if entry.state() == AgentState::Paused { format!("waiting for you · {text}") } else { text };
            (text, color)
        }
    }
}

fn draw_agent_list(app: &mut App, area: Rect, buf: &mut Buffer) {
    let groups = app.groups();
    if groups.is_empty() {
        let lines = [
            ("no agents in this session yet", theme::MUTED),
            ("write a task below and press enter to start one", theme::DIM),
            ("tab switches who runs it, ctrl+l changes where", theme::DIM),
        ];
        for (i, (text, color)) in lines.iter().enumerate() {
            let y = area.y + u16::try_from(i).unwrap_or(0);
            if y < area.bottom() {
                put(buf, area.x, y, text, fg(*color), area.right());
            }
        }
        return;
    }
    let mut lines = Vec::new();
    for group in &groups {
        lines.push(ListLine::Header(group.label.clone(), group.cwd.clone(), group.ids.len()));
        for id in &group.ids {
            lines.push(ListLine::Agent(*id));
            let count = app.agents.get(id).map_or(0, |e| e.subagents.len());
            lines.extend((0..count).map(|index| ListLine::Subagent(*id, index)));
        }
    }
    let height = usize::from(area.height);
    let selected_line =
        app.selected.and_then(|sel| lines.iter().position(|l| matches!(l, ListLine::Agent(id) if *id == sel)));
    if let Some(index) = selected_line {
        if index < app.list_offset {
            app.list_offset = index.saturating_sub(1);
        } else if index >= app.list_offset + height {
            app.list_offset = index + 1 - height;
        }
    }
    app.list_offset = app.list_offset.min(lines.len().saturating_sub(height));

    let name_width = app
        .agents
        .values()
        .filter(|e| e.record.session == app.session)
        .map(|e| width(&e.record.name))
        .max()
        .unwrap_or(NAME_MIN)
        .clamp(NAME_MIN, NAME_MAX);
    let fixed = 8 + name_width + 2 + TIME_WIDTH + 1 + TIME_WIDTH + 2 + KIND_WIDTH;
    let show_uptime = usize::from(area.width) >= fixed + 16;
    let now = now_ms();
    let right = area.right();

    for (row, line) in lines.iter().skip(app.list_offset).take(height).enumerate() {
        let y = area.y + u16::try_from(row).unwrap_or(0);
        match line {
            ListLine::Header(label, cwd, count) => {
                let color = theme::project_color(label);
                let x = put(buf, area.x, y, label, fg(color).add_modifier(Modifier::BOLD), right);
                let x = put(buf, x + 1, y, "›", fg(theme::DIM), right);
                put(buf, x + 1, y, &count.to_string(), fg(theme::FAINT), right);
                app.hits.push((Rect::new(area.x, y, area.width, 1), Hit::Project(cwd.clone())));
            }
            ListLine::Agent(id) => {
                let Some(entry) = app.agents.get(id) else { continue };
                let state = entry.state();
                let selected = app.selected == Some(*id);
                let mut x = area.x;
                x = put(buf, x, y, &format!("{:>3} ", id), fg(theme::DIM), right);
                x = put(buf, x, y, if selected { "▸" } else { " " }, fg(theme::ACCENT), right);
                x = put(buf, x, y, " ", Style::default(), right);
                let (glyph, glyph_color) = if entry.is_starting() {
                    ("◌", theme::DIM)
                } else {
                    theme::glyph(entry.record.kind, state, app.tick)
                };
                x = put(buf, x, y, glyph, fg(glyph_color), right);
                x = put(buf, x, y, " ", Style::default(), right);
                let mut name_style = fg(theme::name_color(entry.record.kind, state));
                if selected {
                    name_style = name_style.add_modifier(Modifier::BOLD);
                }
                x = put(buf, x, y, &pad(&entry.record.name, name_width), name_style, right);
                x = put(buf, x, y, "  ", Style::default(), right);
                let (first, uptime) = time_columns(entry, now);
                let (first_text, first_color) = first.unwrap_or_default();
                x = put(buf, x, y, &pad_left(&first_text, TIME_WIDTH), fg(first_color), right);
                if show_uptime {
                    x = put(buf, x, y, " ", Style::default(), right);
                    x = put(buf, x, y, &pad_left(&uptime.unwrap_or_default(), TIME_WIDTH), fg(theme::DIM), right);
                }
                x = put(buf, x, y, "  ", Style::default(), right);
                x = put(
                    buf,
                    x,
                    y,
                    &pad(entry.record.kind.label(), KIND_WIDTH),
                    fg(theme::kind_color(entry.record.kind)),
                    right,
                );
                let (text, color) = activity(entry);
                let available = usize::from(right.saturating_sub(x));
                put(buf, x, y, &truncate(&text, available), fg(color), right);
                app.hits.push((Rect::new(area.x, y, area.width, 1), Hit::Agent(*id)));
            }
            ListLine::Subagent(parent, index) => {
                let Some(sub) = app.agents.get(parent).and_then(|e| e.subagents.get(*index)) else { continue };
                let mut x = area.x + SUBAGENT_INDENT;
                x = put(buf, x, y, "↳ ", fg(theme::FAINT), right);
                x = put(buf, x, y, theme::SPINNER[app.tick % theme::SPINNER.len()], fg(theme::YELLOW), right);
                x = put(buf, x, y, " ", Style::default(), right);
                let role_width = name_width.saturating_sub(2);
                x = put(buf, x, y, &pad(&truncate(&sub.role, role_width), role_width), fg(theme::MUTED), right);
                x = put(buf, x, y, "  ", Style::default(), right);
                let elapsed = duration_short(now.saturating_sub(sub.started_at_ms));
                x = put(buf, x, y, &pad_left(&elapsed, TIME_WIDTH), fg(theme::DIM), right);
                if show_uptime {
                    x = put(buf, x, y, &" ".repeat(TIME_WIDTH + 1), Style::default(), right);
                }
                x = put(buf, x, y, "  ", Style::default(), right);
                x = put(buf, x, y, &pad(SUBAGENT_LABEL, KIND_WIDTH), fg(theme::FAINT), right);
                let text = match (sub.description.is_empty(), sub.activity.is_empty()) {
                    (false, false) => format!("{} · {}", sub.description, sub.activity),
                    (false, true) => sub.description.clone(),
                    _ => sub.activity.clone(),
                };
                let available = usize::from(right.saturating_sub(x));
                put(buf, x, y, &truncate(&text, available), fg(theme::MUTED), right);
                app.hits.push((Rect::new(area.x, y, area.width, 1), Hit::Agent(*parent)));
            }
        }
    }
}

fn prompt_prefix(app: &App) -> Option<(String, Color, String, usize)> {
    match app.prompt.as_ref()? {
        Prompt::Where(input) => Some(("where › ".into(), theme::LINK, input.text(), width(&input.before_cursor()))),
        Prompt::Rename(id, input) => {
            let old = app.agents.get(id).map(|e| e.record.name.clone()).unwrap_or_default();
            Some((format!("rename {old} › "), theme::ACCENT, input.text(), width(&input.before_cursor())))
        }
        Prompt::NewSession(input) => {
            Some(("new session › ".into(), theme::LINK, input.text(), width(&input.before_cursor())))
        }
        Prompt::RenameSession(input) => {
            Some(("rename session › ".into(), theme::LINK, input.text(), width(&input.before_cursor())))
        }
        Prompt::Confirm(_) => None,
    }
}

fn confirm_text(app: &App, confirm: &Confirm) -> String {
    let name = |id: &AgentId| app.agents.get(id).map(|e| e.record.name.clone()).unwrap_or_default();
    match confirm {
        Confirm::Kill(id) => format!("stop agent {}? the process will be terminated", name(id)),
        Confirm::Remove(id) => format!("remove {} and its recorded output?", name(id)),
        Confirm::Delete(id) => {
            format!("delete {}? the process will be stopped and removed with its recorded output", name(id))
        }
        Confirm::DeleteSession(session) => format!("delete session {session}?  [y/N]"),
        Confirm::Recover(ids) => format!("recover {} interrupted agent(s) in their folders?  [Y/n]", ids.len()),
    }
}

const MORE_RESERVED: usize = 6;

fn first_visible(labels: &[String], selected: Option<usize>, available: usize) -> usize {
    let Some(selected) = selected else { return 0 };
    let mut used = 0;
    let mut start = selected + 1;
    while start > 0 {
        let needed = width(&labels[start - 1]) + 2;
        if used + needed > available {
            break;
        }
        used += needed;
        start -= 1;
    }
    start.min(selected)
}

fn draw_path_suggestions(app: &App, x: u16, y: u16, right: u16, buf: &mut Buffer) {
    let listing = app.paths.listing().map(|dir| app.display(dir)).unwrap_or_default();
    let mut cx = put(buf, x, y, &truncate_start(&listing, 48), fg(theme::FAINT), right);
    cx = put(buf, cx, y, " › ", fg(theme::FAINT), right);
    let suggestions = app.paths.suggestions();
    if suggestions.is_empty() {
        put(buf, cx, y, "no folders here", fg(theme::FAINT), right);
        return;
    }
    let labels: Vec<String> = suggestions.iter().map(|name| format!("{name}/")).collect();
    let available = usize::from(right.saturating_sub(cx)).saturating_sub(MORE_RESERVED);
    let selected = app.paths.selected();
    let start = first_visible(&labels, selected, available);
    let limit = right.saturating_sub(u16::try_from(MORE_RESERVED).unwrap_or(0));
    let mut shown = start;
    for (index, label) in labels.iter().enumerate().skip(start) {
        let needed = u16::try_from(width(label)).unwrap_or(u16::MAX);
        if cx.saturating_add(needed) > limit {
            break;
        }
        let style = if selected == Some(index) {
            fg(theme::ACCENT).bg(theme::TAB_ACTIVE_BG).add_modifier(Modifier::BOLD)
        } else {
            fg(theme::LINK)
        };
        cx = put(buf, cx, y, label, style, right);
        cx = put(buf, cx, y, "  ", Style::default(), right);
        shown = index + 1;
    }
    let hidden = labels.len() - shown + start;
    if hidden > 0 {
        put(buf, cx, y, &format!("+{hidden}"), fg(theme::FAINT), right);
    }
}

fn composer_rows(app: &App) -> u16 {
    match &app.prompt {
        Some(Prompt::Confirm(confirm)) if confirm.is_popup() => {}
        Some(_) => return 1,
        None => {}
    }
    u16::try_from(app.task.line_count()).unwrap_or(MAX_COMPOSER_ROWS).clamp(1, MAX_COMPOSER_ROWS)
}

fn draw_task(app: &App, x: u16, y: u16, rows: u16, right: u16, buf: &mut Buffer) -> Position {
    let text = app.task.text();
    let before = app.task.before_cursor();
    let cursor_row = before.matches('\n').count();
    let cursor_width = width(before.rsplit('\n').next().unwrap_or_default());
    let rows = usize::from(rows.max(1));
    let first = (cursor_row + 1).saturating_sub(rows);
    let visible = usize::from(right.saturating_sub(x + 1));
    let skip = cursor_width.saturating_sub(visible);
    let color = if text.starts_with('/') { theme::LINK } else { theme::TEXT };
    for (index, line) in text.split('\n').enumerate().skip(first).take(rows) {
        let row = y + u16::try_from(index - first).unwrap_or(0);
        let shown: String = line.chars().skip(if index == cursor_row { skip } else { 0 }).collect();
        put(buf, x, row, &shown, fg(color), right);
    }
    let column = x + u16::try_from(cursor_width - skip).unwrap_or(0);
    let row = y + u16::try_from(cursor_row - first).unwrap_or(0);
    Position::new(column.min(right.saturating_sub(1)), row)
}

fn draw_composer(app: &mut App, area: Rect, buf: &mut Buffer) -> Option<Position> {
    if area.height == 0 {
        return None;
    }
    let x = area.x + 1;
    let right = area.right();
    let rows = composer_rows(app).min(area.height);
    let mut cursor = None;
    if let Some(Prompt::Confirm(confirm)) = app.prompt.as_ref()
        && !confirm.is_popup()
    {
        let text = confirm_text(app, confirm);
        put(buf, x, area.y, &text, fg(theme::YELLOW).add_modifier(Modifier::BOLD), right);
    } else if let Some((prefix, color, text, before)) = prompt_prefix(app) {
        let after = put(buf, x, area.y, &prefix, fg(color), right);
        put(buf, after, area.y, &text, fg(theme::TEXT), right);
        let column = after + u16::try_from(before).unwrap_or(0);
        cursor = Some(Position::new(column.min(right.saturating_sub(1)), area.y));
    } else if app.task.is_empty() {
        put(buf, x, area.y, "write a task", fg(theme::FAINT), right);
        cursor = Some(Position::new(x, area.y));
    } else {
        cursor = Some(draw_task(app, x, area.y, rows, right, buf));
    }
    if area.height <= rows {
        return cursor;
    }
    let y = area.y + rows;
    let mut cx = put(buf, x, y, "pc ", fg(theme::DIM), right);
    cx = put(buf, cx, y, &app.hostname, fg(theme::HOST), right);
    cx = put(buf, cx, y, "   ", Style::default(), right);
    let who_start = cx;
    cx = put(buf, cx, y, "who ", fg(theme::DIM), right);
    cx = put(buf, cx, y, app.who.label(), fg(theme::kind_color(app.who)).add_modifier(Modifier::BOLD), right);
    app.hits.push((Rect::new(who_start, y, cx - who_start, 1), Hit::Who));
    cx = put(buf, cx, y, "   ", Style::default(), right);
    let where_start = cx;
    cx = put(buf, cx, y, "where ", fg(theme::DIM), right);
    let where_text = app.display(&app.where_dir);
    cx = put(buf, cx, y, &where_text, fg(theme::LINK), right);
    app.hits.push((Rect::new(where_start, y, cx - where_start, 1), Hit::Where));
    if area.height <= rows + 1 {
        return cursor;
    }
    let y = area.y + rows + 1;
    if app.paths.is_active() {
        draw_path_suggestions(app, x, y, right, buf);
    } else if app.prompt.is_none() && app.task.text().starts_with('/') {
        let typed = app.task.text();
        let word = typed.split_whitespace().next().unwrap_or("/");
        let commands = matching_commands(word).join("  ");
        let mut cx = put(buf, x, y, &commands, fg(theme::MUTED), right);
        let skills = app.skills.matching(app.who, word);
        if !skills.is_empty() {
            if !commands.is_empty() {
                cx = put(buf, cx, y, "  ", Style::default(), right);
            }
            put(buf, cx, y, &skills.join("  "), fg(theme::kind_color(app.who)), right);
        }
    } else if let Some(notice) = app.notice.as_ref() {
        let color = match notice.tone {
            Tone::Info => theme::TEXT,
            Tone::Success => theme::GREEN,
            Tone::Warn => theme::YELLOW,
            Tone::Error => theme::RED,
        };
        put(buf, x, y, &notice.text, fg(color), right);
    } else {
        put(buf, x, y, HINT, fg(theme::FAINT), right);
    }
    cursor
}

fn draw_agent(app: &mut App, id: AgentId, area: Rect, buf: &mut Buffer) -> Option<Position> {
    app.hits.push((area, Hit::Terminal));
    let entry = app.agents.get(&id)?;
    match entry.term.as_ref() {
        Some(term) => term.render(area, buf),
        None => {
            let message = if entry.is_dead() { "loading the last recorded screen…" } else { "connecting…" };
            let y = area.y + area.height / 2;
            let x = area.x + area.width.saturating_sub(u16::try_from(width(message)).unwrap_or(0)) / 2;
            put(buf, x, y, message, fg(theme::DIM), area.right());
            None
        }
    }
}

fn draw_agent_status(app: &App, id: AgentId, area: Rect, buf: &mut Buffer) {
    if area.height == 0 {
        return;
    }
    let Some(entry) = app.agents.get(&id) else { return };
    let right = area.right();
    let y = area.y;
    let scrolled = entry.term.as_ref().is_some_and(|t| t.is_scrolled());
    let hint = if entry.is_dead() {
        "enter restart · esc back · pgup/pgdn scroll"
    } else if scrolled {
        "scrollback · type to return · ctrl+] overview"
    } else {
        "ctrl+] overview · shift+pgup scroll"
    };
    let hint_width = u16::try_from(width(hint)).unwrap_or(0);
    let hint_x = right.saturating_sub(hint_width + 1);
    let limit = if hint_x > area.x + 20 { hint_x.saturating_sub(2) } else { right };
    if limit < right {
        put(buf, hint_x, y, hint, fg(theme::FAINT), right);
    }
    let (state_text, state_color) = match &entry.link {
        Link::Exited(info) => (info.describe(), theme::MUTED),
        Link::Lost => ("interrupted".to_string(), theme::RED),
        _ => {
            let state = entry.state();
            let color = match state {
                AgentState::Working => theme::YELLOW,
                AgentState::Paused => theme::YELLOW,
                AgentState::Done => theme::GREEN,
                _ => theme::MUTED,
            };
            (state.label().to_string(), color)
        }
    };
    let mut x = area.x + 1;
    let (glyph, glyph_color) = theme::glyph(entry.record.kind, entry.state(), app.tick);
    x = put(buf, x, y, glyph, fg(glyph_color), limit);
    x = put(buf, x + 1, y, &entry.record.name, fg(theme::TEXT).add_modifier(Modifier::BOLD), limit);
    x = put(buf, x + 2, y, entry.record.kind.label(), fg(theme::kind_color(entry.record.kind)), limit);
    if entry.record.kind.is_ai() || entry.is_dead() {
        x = put(buf, x + 2, y, &state_text, fg(state_color), limit);
    }
    let path_x = x + 2;
    if path_x < limit {
        let path = truncate(&app.display(&entry.record.cwd), usize::from(limit - path_x));
        put(buf, path_x, y, &path, fg(theme::LINK), limit);
    }
}

fn draw_confirm(app: &App, confirm: &Confirm, screen: Rect, buf: &mut Buffer) {
    let (title, color) = match confirm {
        Confirm::Delete(_) => (" delete agent ", theme::RED),
        Confirm::Remove(_) => (" remove agent ", theme::RED),
        _ => (" stop agent ", theme::YELLOW),
    };
    let width = CONFIRM_WIDTH.min(screen.width.saturating_sub(4));
    let height = CONFIRM_HEIGHT.min(screen.height.saturating_sub(2));
    let area = Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 2,
        width,
        height,
    );
    Clear.render(area, buf);
    let lines = vec![
        Line::default(),
        Line::styled(confirm_text(app, confirm), fg(theme::TEXT).add_modifier(Modifier::BOLD)),
        Line::default(),
        Line::styled(CONFIRM_HINT, fg(theme::DIM)),
    ];
    Paragraph::new(lines)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(fg(color))
                .title(title)
                .title_style(fg(color).add_modifier(Modifier::BOLD)),
        )
        .render(area, buf);
}

const HELP: &[(&str, &str)] = &[
    ("type + enter", "start an agent with that task"),
    ("shift+enter", "new line in the task (alt+enter also works)"),
    ("tab / shift+tab", "who runs it: claude, codex or terminal"),
    ("/where · ctrl+l", "folder for this session: type, tab enters or cycles"),
    ("↑ ↓ / mouse", "select agent · click or enter opens it"),
    ("alt+1..9", "jump to an open tab"),
    ("ctrl+]", "leave an agent and return to the overview"),
    ("shift+pgup/pgdn", "scroll an agent's history (or mouse wheel)"),
    ("ctrl+e / f2", "rename selected agent"),
    ("ctrl+x", "stop a running agent / remove a stopped one"),
    ("delete", "stop and remove the selected agent (asks first)"),
    ("ctrl+r", "recover interrupted agents of this session"),
    ("ctrl+n · alt+↑↓", "new session · switch session"),
    ("/", "commands: /cd /who /session /restart /recover …"),
    ("ctrl+c / ctrl+q", "quit — agents keep running in background"),
];

fn draw_help(screen: Rect, buf: &mut Buffer) {
    let width = 70.min(screen.width.saturating_sub(4));
    let height = (u16::try_from(HELP.len()).unwrap_or(0) + 4).min(screen.height.saturating_sub(2));
    let area = Rect::new(
        screen.x + (screen.width.saturating_sub(width)) / 2,
        screen.y + (screen.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    Clear.render(area, buf);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(key, text)| {
            Line::from(vec![
                ratatui::text::Span::styled(format!(" {key:<18}"), fg(theme::ACCENT)),
                ratatui::text::Span::styled(*text, fg(theme::TEXT)),
            ])
        })
        .collect();
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(fg(theme::RULE))
                .title(" orkai · keys ")
                .title_style(fg(theme::TEXT).add_modifier(Modifier::BOLD)),
        )
        .render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_reserves_sidebar_composer_and_tabs() {
        let screen = Rect::new(0, 0, 120, 40);
        let overview = areas(screen, false, false, 1);
        assert_eq!(overview.bottom.height, 3);
        assert_eq!(overview.rule.y, 36);
        assert!(overview.sidebar.is_some());
        assert_eq!(overview.body.x, SIDEBAR_WIDTH + MAIN_GAP);
        assert!(overview.tabs.is_none());
        let agent = areas(screen, true, true, 1);
        assert_eq!(agent.bottom.height, 1);
        assert_eq!(agent.tabs.unwrap().height, 1);
        assert_eq!(agent.body.height, 40 - 1 - 1 - 1);
        assert_eq!(agent.body.right(), 120);
    }

    #[test]
    fn composer_grows_with_task_lines_up_to_a_limit() {
        let screen = Rect::new(0, 0, 120, 40);
        assert_eq!(areas(screen, false, false, 3).bottom.height, 5);
        assert_eq!(areas(screen, false, false, 40).bottom.height, 2 + MAX_COMPOSER_ROWS);
        assert_eq!(areas(screen, false, false, 0).bottom.height, 3);
        assert_eq!(areas(screen, true, true, 5).bottom.height, 1);
    }

    #[test]
    fn narrow_screens_hide_the_sidebar() {
        let layout = areas(Rect::new(0, 0, 60, 20), false, false, 1);
        assert!(layout.sidebar.is_none());
        assert_eq!(layout.body.x, 1);
    }

    #[test]
    fn usage_bars_round_and_clamp_to_their_width() {
        assert_eq!(usage_cells(0.0, 12), 0);
        assert_eq!(usage_cells(55.0, 12), 7);
        assert_eq!(usage_cells(8.0, 12), 1);
        assert_eq!(usage_cells(100.0, 12), 12);
        assert_eq!(usage_cells(140.0, 12), 12);
        assert_eq!(usage_cells(-5.0, 12), 0);
        assert_eq!(usage_cells(f64::NAN, 12), 0);
    }

    #[test]
    fn tiny_screens_do_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (5, 2), (10, 4)] {
            let _ = areas(Rect::new(0, 0, w, h), true, true, 1);
            let _ = areas(Rect::new(0, 0, w, h), false, true, MAX_COMPOSER_ROWS);
        }
    }
}
