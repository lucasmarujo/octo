use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineInput {
    chars: Vec<char>,
    cursor: usize,
}

impl LineInput {
    pub fn with_text(text: &str) -> Self {
        let mut input = Self::default();
        input.set(text);
        input
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn set(&mut self, text: &str) {
        self.chars = text.chars().filter(|c| !c.is_control()).collect();
        self.cursor = self.chars.len();
    }

    pub fn clear(&mut self) {
        self.chars.clear();
        self.cursor = 0;
    }

    pub fn insert(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, text: &str) {
        for c in text.chars() {
            if c == '\n' || c == '\r' || c == '\t' {
                self.insert(' ');
            } else {
                self.insert(c);
            }
        }
    }

    pub fn insert_newline(&mut self) {
        self.chars.insert(self.cursor, '\n');
        self.cursor += 1;
    }

    pub fn insert_multiline(&mut self, text: &str) {
        for c in text.replace("\r\n", "\n").chars() {
            match c {
                '\n' | '\r' => self.insert_newline(),
                '\t' => self.insert(' '),
                c => self.insert(c),
            }
        }
    }

    pub fn line_count(&self) -> usize {
        self.chars.iter().filter(|c| **c == '\n').count() + 1
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    pub fn delete_word(&mut self) {
        while self.cursor > 0 && self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
        while self.cursor > 0 && !self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    /// Moves the cursor to the start of the word on its left.
    pub fn left_word(&mut self) {
        while self.cursor > 0 && self.chars[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
        }
        while self.cursor > 0 && !self.chars[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
        }
    }

    /// Moves the cursor to the end of the word on its right.
    pub fn right_word(&mut self) {
        if self.cursor < self.chars.len() && self.chars[self.cursor].is_whitespace() {
            while self.cursor < self.chars.len() && self.chars[self.cursor].is_whitespace() {
                self.cursor += 1;
            }
        }
        while self.cursor < self.chars.len() && !self.chars[self.cursor].is_whitespace() {
            self.cursor += 1;
        }
    }

    /// Puts the cursor at the given index, clamped to the end of the text.
    pub fn set_cursor(&mut self, index: usize) {
        self.cursor = index.min(self.chars.len());
    }

    /// The index of the character shown at the given display column of a line.
    ///
    /// The column is counted in terminal cells and is clamped to the end of the line.
    pub fn index_at(&self, line: usize, column: usize) -> Option<usize> {
        let mut current = 0usize;
        let mut start = 0usize;
        for (index, c) in self.chars.iter().enumerate() {
            if current == line {
                start = index;
                break;
            }
            if *c == '\n' {
                current += 1;
                start = index + 1;
            }
        }
        if current < line {
            return None;
        }
        let end = self.chars[start..].iter().position(|c| *c == '\n').map_or(self.chars.len(), |at| start + at);
        let mut used = 0usize;
        for index in start..end {
            let w = UnicodeWidthChar::width(self.chars[index]).unwrap_or(0);
            if used + w > column {
                return Some(if w == 2 && column == used + 1 { index + 1 } else { index });
            }
            used += w;
        }
        Some(end)
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    pub fn before_cursor(&self) -> String {
        self.chars[..self.cursor].iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_operations() {
        let mut input = LineInput::default();
        for c in "helo".chars() {
            input.insert(c);
        }
        input.left();
        input.insert('l');
        assert_eq!(input.text(), "hello");
        input.home();
        input.delete();
        assert_eq!(input.text(), "ello");
        input.end();
        input.backspace();
        assert_eq!(input.text(), "ell");
        assert_eq!(input.before_cursor(), "ell");
    }

    #[test]
    fn delete_word_removes_previous_word() {
        let mut input = LineInput::with_text("fix the  bug ");
        input.delete_word();
        assert_eq!(input.text(), "fix the  ");
    }

    #[test]
    fn pasted_newlines_become_spaces_and_controls_are_dropped() {
        let mut input = LineInput::default();
        input.insert_str("a\nb\tc");
        input.insert('\u{7}');
        assert_eq!(input.text(), "a b c");
        assert_eq!(LineInput::with_text("x\u{1b}y").text(), "xy");
    }

    #[test]
    fn multiline_input_keeps_line_breaks() {
        let mut input = LineInput::with_text("fix");
        input.insert_newline();
        input.insert_multiline("a\r\nb\rc\td\u{7}");
        assert_eq!(input.text(), "fix\na\nb\nc d");
        assert_eq!(input.line_count(), 4);
        input.backspace();
        input.backspace();
        input.backspace();
        input.backspace();
        assert_eq!(input.text(), "fix\na\nb");
        assert_eq!(LineInput::with_text("x\ny").text(), "xy");
        assert_eq!(LineInput::default().line_count(), 1);
    }

    #[test]
    fn cursor_is_bounded() {
        let mut input = LineInput::with_text("ab");
        input.right();
        input.right();
        assert_eq!(input.before_cursor(), "ab");
        input.home();
        input.left();
        input.backspace();
        assert_eq!(input.text(), "ab");
    }

    #[test]
    fn words_are_skipped_one_at_a_time() {
        let mut input = LineInput::with_text("fix the login bug");
        input.left_word();
        assert_eq!(input.before_cursor(), "fix the login ");
        input.left_word();
        assert_eq!(input.before_cursor(), "fix the ");
        input.left_word();
        assert_eq!(input.before_cursor(), "fix ");
        input.left_word();
        assert_eq!(input.before_cursor(), "");
        input.left_word();
        assert_eq!(input.before_cursor(), "");
    }

    #[test]
    fn the_word_on_the_right_stops_at_its_end() {
        let mut input = LineInput::with_text("fix the login bug");
        input.set_cursor(0);
        input.right_word();
        assert_eq!(input.before_cursor(), "fix");
        input.right_word();
        assert_eq!(input.before_cursor(), "fix the");
        input.right_word();
        assert_eq!(input.before_cursor(), "fix the login");
        input.right_word();
        assert_eq!(input.before_cursor(), "fix the login bug");
        input.right_word();
        assert_eq!(input.before_cursor(), "fix the login bug");
    }

    #[test]
    fn a_click_finds_the_character_under_the_column() {
        let input = LineInput::with_text("fix the login bug");
        assert_eq!(input.index_at(0, 0), Some(0));
        assert_eq!(input.index_at(0, 3), Some(3));
        assert_eq!(input.index_at(0, 4), Some(4));
        assert_eq!(input.index_at(0, 99), Some(17));
    }

    #[test]
    fn a_click_on_a_later_line_finds_that_line() {
        let mut input = LineInput::default();
        input.insert_multiline("first\nsecond\nthird");
        assert_eq!(input.index_at(1, 2), Some(8));
        assert_eq!(input.index_at(2, 0), Some(13));
        assert_eq!(input.index_at(9, 0), None);
    }

    #[test]
    fn a_click_counts_wide_characters_as_two_columns() {
        let input = LineInput::with_text("漢字abc");
        assert_eq!(input.index_at(0, 0), Some(0));
        assert_eq!(input.index_at(0, 1), Some(1));
        assert_eq!(input.index_at(0, 2), Some(1));
        assert_eq!(input.index_at(0, 3), Some(2));
        assert_eq!(input.index_at(0, 4), Some(2));
    }
}
