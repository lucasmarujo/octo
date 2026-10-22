use std::fmt::Write as _;

use qrcode::{Color, EcLevel, QrCode};

pub const QUIET_ZONE: usize = 2;
pub const DARK: (u8, u8, u8) = (18, 18, 24);
pub const LIGHT: (u8, u8, u8) = (236, 236, 240);
pub const UPPER_HALF: &str = "▀";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Qr {
    size: usize,
    dark: Vec<bool>,
}

impl Qr {
    pub fn encode(text: &str) -> Option<Self> {
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
        let width = code.width();
        let size = width + 2 * QUIET_ZONE;
        let mut dark = vec![false; size * size];
        for (index, color) in code.to_colors().into_iter().enumerate() {
            let (x, y) = (index % width + QUIET_ZONE, index / width + QUIET_ZONE);
            dark[y * size + x] = color == Color::Dark;
        }
        Some(Self { size, dark })
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn rows(&self) -> usize {
        self.size.div_ceil(2)
    }

    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.dark[y * self.size + x]
    }

    pub fn cell(&self, x: usize, row: usize) -> (bool, bool) {
        (self.is_dark(x, row * 2), self.is_dark(x, row * 2 + 1))
    }

    pub fn ansi_lines(&self) -> Vec<String> {
        let rgb = |dark: bool| if dark { DARK } else { LIGHT };
        (0..self.rows())
            .map(|row| {
                let mut line = String::new();
                for x in 0..self.size {
                    let (top, bottom) = self.cell(x, row);
                    let ((tr, tg, tb), (br, bg, bb)) = (rgb(top), rgb(bottom));
                    let _ = write!(line, "\x1b[38;2;{tr};{tg};{tb};48;2;{br};{bg};{bb}m{UPPER_HALF}");
                }
                line.push_str("\x1b[0m");
                line
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "https://quiet-river-sound-lamp.trycloudflare.com/#k=Qm9vdHN0cmFwcGluZyB0aGUgb3JrYWkgcGFpcmluZw";

    #[test]
    fn the_code_is_square_with_a_light_quiet_zone() {
        let qr = Qr::encode(LINK).unwrap();
        assert!(qr.size() >= 21 + 2 * QUIET_ZONE);
        for i in 0..qr.size() {
            for edge in 0..QUIET_ZONE {
                assert!(!qr.is_dark(i, edge) && !qr.is_dark(edge, i), "the quiet zone must stay light");
            }
        }
        assert!(qr.is_dark(QUIET_ZONE, QUIET_ZONE), "the finder pattern starts in the corner");
        assert!(!qr.is_dark(qr.size(), 0), "outside the code is light");
    }

    #[test]
    fn two_modules_fit_in_one_terminal_row() {
        let qr = Qr::encode(LINK).unwrap();
        assert_eq!(qr.rows(), qr.size().div_ceil(2));
        assert_eq!(qr.cell(QUIET_ZONE, 1), (qr.is_dark(QUIET_ZONE, 2), qr.is_dark(QUIET_ZONE, 3)));
    }

    #[test]
    fn a_pairing_link_fits_a_terminal() {
        let qr = Qr::encode(LINK).unwrap();
        assert!(qr.rows() <= 26, "{} rows", qr.rows());
        assert!(qr.size() <= 52, "{} columns", qr.size());
    }

    #[test]
    fn the_terminal_lines_paint_both_colors_explicitly() {
        let qr = Qr::encode("orkai").unwrap();
        let lines = qr.ansi_lines();
        assert_eq!(lines.len(), qr.rows());
        for line in &lines {
            assert_eq!(line.matches(UPPER_HALF).count(), qr.size());
            assert!(line.ends_with("\x1b[0m"));
        }
        assert!(lines[0].contains("38;2;236;236;240;48;2;236;236;240m"), "the first row is quiet zone");
    }
}
