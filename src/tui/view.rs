use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

use crate::protocol::TermSize;

pub const CLIENT_SCROLLBACK: usize = 3_000;

pub struct TermView {
    parser: vt100::Parser,
    scroll: usize,
}

impl TermView {
    pub fn new(size: TermSize) -> Self {
        Self { parser: vt100::Parser::new(size.rows, size.cols, CLIENT_SCROLLBACK), scroll: 0 }
    }

    pub fn from_snapshot(size: TermSize, data: &[u8]) -> Self {
        let mut view = Self::new(size);
        view.parser.process(data);
        view
    }

    pub fn from_parser(parser: vt100::Parser) -> Self {
        Self { parser, scroll: 0 }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        if self.scroll > 0 {
            self.parser.screen_mut().set_scrollback(self.scroll);
            self.scroll = self.parser.screen().scrollback();
        }
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn size(&self) -> TermSize {
        let (rows, cols) = self.parser.screen().size();
        TermSize { rows, cols }
    }

    pub fn resize(&mut self, size: TermSize) {
        self.parser.screen_mut().set_size(size.rows, size.cols);
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let target = if delta >= 0 {
            self.scroll.saturating_add(delta.unsigned_abs())
        } else {
            self.scroll.saturating_sub(delta.unsigned_abs())
        };
        self.parser.screen_mut().set_scrollback(target);
        self.scroll = self.parser.screen().scrollback();
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = 0;
        self.parser.screen_mut().set_scrollback(0);
    }

    pub fn is_scrolled(&self) -> bool {
        self.scroll > 0
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) -> Option<Position> {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let height = area.height.min(rows);
        let width = area.width.min(cols);
        for row in 0..height {
            let mut col = 0;
            while col < width {
                let Some(cell) = screen.cell(row, col) else {
                    col += 1;
                    continue;
                };
                let Some(target) = buf.cell_mut((area.x + col, area.y + row)) else {
                    col += 1;
                    continue;
                };
                if cell.is_wide_continuation() {
                    target.reset();
                    col += 1;
                    continue;
                }
                let fits = !cell.is_wide() || col + 1 < width;
                let symbol = if cell.has_contents() && fits { cell.contents() } else { " " };
                target.set_symbol(symbol).set_style(cell_style(cell));
                col += 1;
            }
        }
        if self.scroll > 0 || screen.hide_cursor() {
            return None;
        }
        let (row, col) = screen.cursor_position();
        (row < height && col < width).then(|| Position::new(area.x + col, area.y + row))
    }
}

fn convert(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut modifiers = Modifier::empty();
    if cell.bold() {
        modifiers |= Modifier::BOLD;
    }
    if cell.dim() {
        modifiers |= Modifier::DIM;
    }
    if cell.italic() {
        modifiers |= Modifier::ITALIC;
    }
    if cell.underline() {
        modifiers |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        modifiers |= Modifier::REVERSED;
    }
    Style::default().fg(convert(cell.fgcolor())).bg(convert(cell.bgcolor())).add_modifier(modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_text_colors_and_cursor() {
        let mut view = TermView::new(TermSize { rows: 3, cols: 10 });
        view.process(b"\x1b[32mok\x1b[0m\r\nnext");
        let area = Rect::new(2, 1, 10, 3);
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 5));
        let cursor = view.render(area, &mut buf);
        assert_eq!(buf[(2, 1)].symbol(), "o");
        assert_eq!(buf[(2, 1)].fg, Color::Indexed(2));
        assert_eq!(buf[(2, 2)].symbol(), "n");
        assert_eq!(cursor, Some(Position::new(6, 2)));
    }

    #[test]
    fn scrolling_hides_cursor_and_shows_history() {
        let mut view = TermView::new(TermSize { rows: 2, cols: 10 });
        view.process(b"one\r\ntwo\r\nthree\r\nfour");
        view.scroll_by(2);
        assert!(view.is_scrolled());
        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        assert_eq!(view.render(area, &mut buf), None);
        assert_eq!(buf[(0, 0)].symbol(), "o");
        view.scroll_to_bottom();
        assert!(!view.is_scrolled());
    }

    #[test]
    fn render_clips_to_area() {
        let mut view = TermView::new(TermSize { rows: 5, cols: 20 });
        view.process(b"abcdefghijklmnopqrst");
        let area = Rect::new(0, 0, 4, 1);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        assert_eq!(buf[(3, 0)].symbol(), "d");
    }

    #[test]
    fn snapshot_view_matches_source() {
        let mut source = vt100::Parser::new(3, 10, 0);
        source.process(b"hello");
        let data = crate::termfmt::snapshot(source.screen_mut());
        let view = TermView::from_snapshot(TermSize { rows: 3, cols: 10 }, &data);
        assert_eq!(view.screen().contents(), "hello");
    }
}
