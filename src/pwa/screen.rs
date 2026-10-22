use crate::model::AgentId;
use crate::pwa::protocol::{Run, Screen};
use crate::termfmt;
use crate::util::fnv1a64;

pub const DEFAULT_COLOR: i32 = -1;
pub const TRUECOLOR: i32 = 256;
pub const BOLD: u8 = 1;
pub const DIM: u8 = 2;
pub const ITALIC: u8 = 4;
pub const UNDERLINE: u8 = 8;
pub const INVERSE: u8 = 16;

pub fn color(color: vt100::Color) -> i32 {
    match color {
        vt100::Color::Default => DEFAULT_COLOR,
        vt100::Color::Idx(index) => i32::from(index),
        vt100::Color::Rgb(r, g, b) => TRUECOLOR + ((i32::from(r) << 16) | (i32::from(g) << 8) | i32::from(b)),
    }
}

fn flags(cell: &vt100::Cell) -> u8 {
    [(cell.bold(), BOLD), (cell.dim(), DIM), (cell.italic(), ITALIC), (cell.underline(), UNDERLINE)]
        .into_iter()
        .chain([(cell.inverse(), INVERSE)])
        .filter(|(on, _)| *on)
        .fold(0, |acc, (_, flag)| acc | flag)
}

pub fn row(screen: &vt100::Screen, row: u16) -> Vec<Run> {
    let (_, cols) = screen.size();
    let mut end = cols;
    while end > 0 && screen.cell(row, end - 1).is_none_or(termfmt::is_blank) {
        end -= 1;
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut col = 0;
    while col < end {
        let Some(cell) = screen.cell(row, col) else { break };
        if cell.is_wide_continuation() {
            col += 1;
            continue;
        }
        let (fg, bg, flags) = (color(cell.fgcolor()), color(cell.bgcolor()), flags(cell));
        let text = if cell.has_contents() { cell.contents() } else { " " };
        match runs.last_mut() {
            Some(run) if (run.1, run.2, run.3) == (fg, bg, flags) => run.0.push_str(text),
            _ => runs.push((text.to_string(), fg, bg, flags)),
        }
        col += if cell.is_wide() { 2 } else { 1 };
    }
    runs
}

fn fingerprint(runs: &[Run]) -> u64 {
    let mut bytes = Vec::new();
    for (text, fg, bg, flags) in runs {
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0xff);
        bytes.extend_from_slice(&fg.to_le_bytes());
        bytes.extend_from_slice(&bg.to_le_bytes());
        bytes.push(*flags);
    }
    fnv1a64(&bytes)
}

#[derive(Default)]
pub struct Mirror {
    rows: Vec<u64>,
    size: (u16, u16),
    cursor: Option<(u16, u16)>,
    scroll: usize,
    primed: bool,
}

impl Mirror {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn update(&mut self, agent: AgentId, screen: &vt100::Screen, scroll: usize) -> Option<Screen> {
        let (rows, cols) = screen.size();
        let full = !self.primed || self.size != (rows, cols) || self.scroll != scroll;
        if full {
            self.rows = vec![0; usize::from(rows)];
        }
        let mut lines = Vec::new();
        for (index, known) in (0..rows).zip(self.rows.iter_mut()) {
            let runs = row(screen, index);
            let print = fingerprint(&runs);
            if full || *known != print {
                *known = print;
                lines.push((index, runs));
            }
        }
        let cursor = (scroll == 0 && !screen.hide_cursor()).then(|| screen.cursor_position());
        if !full && lines.is_empty() && cursor == self.cursor {
            return None;
        }
        self.primed = true;
        self.size = (rows, cols);
        self.cursor = cursor;
        self.scroll = scroll;
        Some(Screen { agent, rows, cols, full, cursor, scroll, lines })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(bytes: &[u8]) -> vt100::Parser {
        let mut parser = vt100::Parser::new(4, 20, 0);
        parser.process(bytes);
        parser
    }

    #[test]
    fn colors_keep_the_default_the_palette_and_true_color_apart() {
        assert_eq!(color(vt100::Color::Default), DEFAULT_COLOR);
        assert_eq!(color(vt100::Color::Idx(9)), 9);
        assert_eq!(color(vt100::Color::Idx(255)), 255);
        assert_eq!(color(vt100::Color::Rgb(0, 0, 0)), TRUECOLOR);
        assert_eq!(color(vt100::Color::Rgb(255, 95, 135)), TRUECOLOR + 0xff5f87);
    }

    #[test]
    fn a_row_groups_cells_of_the_same_style_and_drops_the_blank_tail() {
        let parser = screen(b"ab\x1b[1;31mcd\x1b[0m e   ");
        let runs = row(parser.screen(), 0);
        assert_eq!(
            runs,
            vec![
                ("ab".into(), DEFAULT_COLOR, DEFAULT_COLOR, 0),
                ("cd".into(), 1, DEFAULT_COLOR, BOLD),
                (" e".into(), DEFAULT_COLOR, DEFAULT_COLOR, 0)
            ]
        );
        assert!(row(parser.screen(), 3).is_empty());
    }

    #[test]
    fn a_coloured_background_at_the_end_of_a_row_is_kept() {
        let parser = screen(b"x\x1b[44m   \x1b[0m");
        let runs = row(parser.screen(), 0);
        assert_eq!(runs.last().unwrap(), &("   ".to_string(), DEFAULT_COLOR, 4, 0));
    }

    #[test]
    fn inverse_and_underline_are_flags() {
        let parser = screen(b"\x1b[7;4;3;2mz");
        assert_eq!(row(parser.screen(), 0)[0].3, INVERSE | UNDERLINE | ITALIC | DIM);
    }

    #[test]
    fn a_wide_character_is_one_cell_of_text() {
        let parser = screen("日本".as_bytes());
        assert_eq!(row(parser.screen(), 0), vec![("日本".to_string(), DEFAULT_COLOR, DEFAULT_COLOR, 0)]);
    }

    #[test]
    fn the_first_update_is_full_and_later_ones_carry_only_changed_rows() {
        let mut parser = screen(b"one\r\ntwo");
        let mut mirror = Mirror::default();
        let first = mirror.update(7, parser.screen(), 0).unwrap();
        assert!(first.full);
        assert_eq!(first.lines.len(), 4);
        assert_eq!((first.rows, first.cols, first.agent), (4, 20, 7));
        assert!(mirror.update(7, parser.screen(), 0).is_none(), "nothing changed");
        parser.process(b"\r\nthree");
        let next = mirror.update(7, parser.screen(), 0).unwrap();
        assert!(!next.full);
        assert_eq!(next.lines.iter().map(|(row, _)| *row).collect::<Vec<_>>(), vec![2]);
        assert_eq!(next.cursor, Some((2, 5)));
    }

    #[test]
    fn a_resize_or_a_scroll_sends_the_whole_screen_again() {
        let mut parser = screen(b"one");
        let mut mirror = Mirror::default();
        mirror.update(1, parser.screen(), 0).unwrap();
        parser.screen_mut().set_size(6, 30);
        assert!(mirror.update(1, parser.screen(), 0).unwrap().full);
        let scrolled = mirror.update(1, parser.screen(), 2).unwrap();
        assert!(scrolled.full);
        assert_eq!(scrolled.cursor, None, "a scrolled screen has no cursor");
        mirror.reset();
        assert!(mirror.update(1, parser.screen(), 2).unwrap().full);
    }

    #[test]
    fn a_hidden_cursor_is_not_sent() {
        let parser = screen(b"\x1b[?25lhidden");
        let mut mirror = Mirror::default();
        assert_eq!(mirror.update(1, parser.screen(), 0).unwrap().cursor, None);
    }
}
