use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellStyle {
    fg: vt100::Color,
    bg: vt100::Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl CellStyle {
    fn of(cell: &vt100::Cell) -> Self {
        Self {
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            bold: cell.bold(),
            dim: cell.dim(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }

    fn write_sgr(&self, out: &mut Vec<u8>) {
        let mut sgr = String::from("\x1b[0");
        for (enabled, code) in
            [(self.bold, "1"), (self.dim, "2"), (self.italic, "3"), (self.underline, "4"), (self.inverse, "7")]
        {
            if enabled {
                sgr.push(';');
                sgr.push_str(code);
            }
        }
        push_color(&mut sgr, self.fg, 30, 90, 38);
        push_color(&mut sgr, self.bg, 40, 100, 48);
        sgr.push('m');
        out.extend_from_slice(sgr.as_bytes());
    }
}

fn push_color(sgr: &mut String, color: vt100::Color, base: u8, bright: u8, extended: u8) {
    match color {
        vt100::Color::Default => {}
        vt100::Color::Idx(i) if i < 8 => {
            let _ = write!(sgr, ";{}", base + i);
        }
        vt100::Color::Idx(i) if i < 16 => {
            let _ = write!(sgr, ";{}", bright + (i - 8));
        }
        vt100::Color::Idx(i) => {
            let _ = write!(sgr, ";{extended};5;{i}");
        }
        vt100::Color::Rgb(r, g, b) => {
            let _ = write!(sgr, ";{extended};2;{r};{g};{b}");
        }
    }
}

pub fn is_blank(cell: &vt100::Cell) -> bool {
    (!cell.has_contents() || cell.contents() == " ")
        && cell.bgcolor() == vt100::Color::Default
        && !cell.inverse()
        && !cell.underline()
}

pub fn write_row(screen: &vt100::Screen, row: u16, cols: u16, out: &mut Vec<u8>) {
    let mut end = cols;
    while end > 0 && screen.cell(row, end - 1).is_none_or(is_blank) {
        end -= 1;
    }
    let mut current: Option<CellStyle> = None;
    let mut col = 0;
    while col < end {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            col += 1;
            continue;
        }
        let style = CellStyle::of(cell);
        if current != Some(style) {
            style.write_sgr(out);
            current = Some(style);
        }
        if cell.has_contents() {
            out.extend_from_slice(cell.contents().as_bytes());
        } else {
            out.push(b' ');
        }
        col += if cell.is_wide() { 2 } else { 1 };
    }
    if current.is_some() {
        out.extend_from_slice(b"\x1b[m");
    }
}

pub fn snapshot(screen: &mut vt100::Screen) -> Vec<u8> {
    let (rows, cols) = screen.size();
    let mut out = Vec::new();
    if screen.alternate_screen() {
        out.extend_from_slice(b"\x1b[?1049h");
    } else {
        screen.set_scrollback(usize::MAX);
        let total = screen.scrollback();
        for offset in (1..=total).rev() {
            screen.set_scrollback(offset);
            write_row(screen, 0, cols, &mut out);
            out.extend_from_slice(b"\r\n");
        }
        screen.set_scrollback(0);
        if total > 0 {
            for _ in 1..rows {
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(&screen.state_formatted());
    out
}

pub fn screen_tail(screen: &vt100::Screen, rows: usize) -> String {
    let (_, cols) = screen.size();
    let all: Vec<String> = screen.rows(0, cols).collect();
    let start = all.len().saturating_sub(rows);
    all[start..].join("\n")
}

pub fn transcript(screen: &mut vt100::Screen) -> Vec<String> {
    let (_, cols) = screen.size();
    let mut lines = Vec::new();
    screen.set_scrollback(usize::MAX);
    let total = screen.scrollback();
    for offset in (1..=total).rev() {
        screen.set_scrollback(offset);
        lines.push(screen.rows(0, cols).next().unwrap_or_default());
    }
    screen.set_scrollback(0);
    lines.extend(screen.rows(0, cols));
    let mut lines: Vec<String> = lines.into_iter().map(|l| l.trim_end().to_string()).collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser(rows: u16, cols: u16, scrollback: usize, input: &[u8]) -> vt100::Parser {
        let mut p = vt100::Parser::new(rows, cols, scrollback);
        p.process(input);
        p
    }

    #[test]
    fn snapshot_reproduces_visible_screen() {
        let mut source = parser(4, 20, 100, b"\x1b[31mred\x1b[0m plain\r\nline two\r\n\x1b[1mbold");
        let data = snapshot(source.screen_mut());
        let mut replica = vt100::Parser::new(4, 20, 100);
        replica.process(&data);
        assert_eq!(replica.screen().contents(), source.screen().contents());
        assert_eq!(replica.screen().cursor_position(), source.screen().cursor_position());
        assert_eq!(replica.screen().cell(0, 0).unwrap().fgcolor(), vt100::Color::Idx(1));
        assert!(replica.screen().cell(2, 0).unwrap().bold());
    }

    #[test]
    fn snapshot_carries_scrollback_history() {
        let mut input = Vec::new();
        for i in 0..10 {
            input.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        input.extend_from_slice(b"prompt>");
        let mut source = parser(3, 20, 100, &input);
        let data = snapshot(source.screen_mut());
        let mut replica = vt100::Parser::new(3, 20, 100);
        replica.process(&data);
        assert_eq!(replica.screen().contents(), source.screen().contents());
        let history = transcript(replica.screen_mut());
        assert_eq!(history.first().map(String::as_str), Some("line 0"));
        assert!(history.contains(&"line 7".to_string()));
        assert_eq!(history.last().map(String::as_str), Some("prompt>"));
        assert_eq!(source.screen().scrollback(), 0);
    }

    #[test]
    fn snapshot_preserves_input_modes() {
        let mut source = parser(3, 10, 0, b"\x1b[?2004h\x1b[?1h");
        let mut replica = vt100::Parser::new(3, 10, 0);
        replica.process(&snapshot(source.screen_mut()));
        assert!(replica.screen().bracketed_paste());
        assert!(replica.screen().application_cursor());
    }

    #[test]
    fn snapshot_of_alternate_screen_enters_alternate_screen() {
        let mut source = parser(3, 10, 10, b"main\r\n\x1b[?1049hfull screen app");
        let mut replica = vt100::Parser::new(3, 10, 10);
        replica.process(&snapshot(source.screen_mut()));
        assert!(replica.screen().alternate_screen());
        assert_eq!(replica.screen().contents(), source.screen().contents());
    }

    #[test]
    fn write_row_trims_trailing_blanks_and_keeps_wide_chars() {
        let source = parser(2, 12, 0, "漢字 ok   ".as_bytes());
        let mut out = Vec::new();
        write_row(source.screen(), 0, 12, &mut out);
        let mut replica = vt100::Parser::new(2, 12, 0);
        replica.process(&out);
        assert_eq!(replica.screen().contents(), "漢字 ok");
    }

    #[test]
    fn rgb_and_indexed_colors_survive_round_trip() {
        let source = parser(2, 10, 0, b"\x1b[38;2;10;20;30;48;5;200mX");
        let mut out = Vec::new();
        write_row(source.screen(), 0, 10, &mut out);
        let mut replica = vt100::Parser::new(2, 10, 0);
        replica.process(&out);
        let cell = replica.screen().cell(0, 0).unwrap();
        assert_eq!(cell.fgcolor(), vt100::Color::Rgb(10, 20, 30));
        assert_eq!(cell.bgcolor(), vt100::Color::Idx(200));
    }

    #[test]
    fn screen_tail_returns_last_rows() {
        let source = parser(4, 10, 0, b"a\r\nb\r\nc\r\nd");
        assert_eq!(screen_tail(source.screen(), 2), "c\nd");
    }
}
