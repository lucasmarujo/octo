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
}
