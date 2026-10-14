use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

pub const WIDTH: u16 = 14;
pub const HEIGHT: u16 = 8;

const IDLE: [&str; 16] = [
    "..tf..........",
    "..tff..ccc....",
    "..tfffccccc...",
    "..tf.CCCCCCC..",
    "..t...sssss...",
    "..t...seses...",
    "..t...sssss...",
    "..t...shhhs...",
    "..t..nnnnnnnn.",
    ".stsnwwwwwwwn.",
    "..t..nnnnnnnn.",
    "..t..wwwwwwws.",
    "..t..ppppppp..",
    "..t..ppp.ppp..",
    "..t..ppp.ppp..",
    "..t.ooo..ooo..",
];

const BUSY: [&str; 16] = [
    "..tff.........",
    "..tfff.ccc....",
    "..tff.ccccc...",
    "..t..CCCCCCC..",
    "..t...sssss...",
    "..t...seses...",
    "..t...sssss...",
    "..t...shhhs...",
    "..t..nnnnnnnn.",
    ".stsnwwwwwwwn.",
    "..t..nnnnnnnn.",
    "..t..wwwwwwws.",
    "..t..ppppppp..",
    "..t..ppp.ppp..",
    "..t..ppp.ppp..",
    "..t.ooo..ooo..",
];

fn color(code: u8) -> Option<Color> {
    Some(match code {
        b't' => Color::Rgb(156, 102, 54),
        b'f' => Color::Rgb(64, 200, 210),
        b'c' => Color::Rgb(44, 60, 120),
        b'C' => Color::Rgb(236, 236, 242),
        b's' => Color::Rgb(242, 192, 142),
        b'e' => Color::Rgb(28, 28, 34),
        b'h' => Color::Rgb(170, 106, 52),
        b'w' => Color::Rgb(242, 242, 248),
        b'n' => Color::Rgb(56, 86, 168),
        b'p' => Color::Rgb(64, 64, 96),
        b'o' => Color::Rgb(116, 70, 38),
        _ => return None,
    })
}

fn pixel(frame: &[&str; 16], row: usize, col: usize) -> Option<Color> {
    frame.get(row).and_then(|line| line.as_bytes().get(col)).copied().and_then(color)
}

pub fn render(area: Rect, buf: &mut Buffer, busy: bool, tick: usize) {
    let frame = if busy && tick % 2 == 1 { &BUSY } else { &IDLE };
    for row in 0..HEIGHT.min(area.height) {
        for col in 0..WIDTH.min(area.width) {
            let top = pixel(frame, usize::from(row) * 2, usize::from(col));
            let bottom = pixel(frame, usize::from(row) * 2 + 1, usize::from(col));
            let (symbol, style) = match (top, bottom) {
                (None, None) => continue,
                (Some(t), None) => ("▀", Style::default().fg(t)),
                (None, Some(b)) => ("▄", Style::default().fg(b)),
                (Some(t), Some(b)) if t == b => ("█", Style::default().fg(t)),
                (Some(t), Some(b)) => ("▀", Style::default().fg(t).bg(b)),
            };
            if let Some(cell) = buf.cell_mut((area.x + col, area.y + row)) {
                cell.set_symbol(symbol).set_style(style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_have_expected_dimensions_and_palette() {
        for frame in [&IDLE, &BUSY] {
            for line in frame.iter() {
                assert_eq!(line.len(), usize::from(WIDTH), "{line}");
                assert!(line.bytes().all(|b| b == b'.' || color(b).is_some()), "{line}");
            }
        }
    }

    #[test]
    fn renders_into_buffer_without_overflow() {
        let area = Rect::new(0, 0, WIDTH, HEIGHT);
        let mut buf = Buffer::empty(area);
        render(area, &mut buf, false, 0);
        let painted = buf.content().iter().filter(|c| c.symbol() != " ").count();
        assert!(painted > 40);
        let small = Rect::new(0, 0, 4, 2);
        let mut small_buf = Buffer::empty(small);
        render(small, &mut small_buf, true, 1);
    }
}
