use std::path::{Component, Path};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "to", "of", "and", "or", "for", "in", "on", "at", "with", "by", "from", "into", "is", "it",
    "this", "that", "please", "pls", "can", "you", "de", "da", "do", "das", "dos", "o", "os", "as", "e", "um", "uma",
    "para", "pra", "em", "no", "na", "nos", "nas", "com", "que", "por", "se", "eu", "voce", "me", "meu", "minha",
    "favor",
];

pub fn duration_short(ms: u64) -> String {
    let secs = ms / 1_000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else if secs < 100 * 3_600 {
        format!("{}h{:02}m", secs / 3_600, (secs % 3_600) / 60)
    } else {
        format!("{}d{:02}h", secs / 86_400, (secs % 86_400) / 3_600)
    }
}

fn fold_char(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

pub fn slugify(text: &str, max_words: usize, max_len: usize) -> String {
    let mut slug = String::new();
    let words = text
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| {
            w.chars()
                .flat_map(char::to_lowercase)
                .map(fold_char)
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()))
        .take(max_words);
    for word in words {
        let extra = if slug.is_empty() { word.len() } else { word.len() + 1 };
        if slug.len() + extra > max_len {
            if slug.is_empty() {
                slug.push_str(&word[..max_len.min(word.len())]);
            }
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word);
    }
    slug
}

pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if let Some(home) = home {
        let home_text = home.to_string_lossy().replace('\\', "/");
        let home_text = home_text.trim_end_matches('/');
        let matches = if cfg!(windows) {
            text.to_lowercase().starts_with(&home_text.to_lowercase())
        } else {
            text.starts_with(home_text)
        };
        if !home_text.is_empty() && matches {
            let rest = &text[home_text.len()..];
            if rest.is_empty() {
                return "~".to_string();
            }
            if rest.starts_with('/') {
                return format!("~{rest}");
            }
        }
    }
    text
}

pub fn project_label(path: &Path) -> String {
    let names: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    match names.len() {
        0 => path.to_string_lossy().into_owned(),
        1 => names[0].clone(),
        n => format!("{}/{}", names[n - 2], names[n - 1]),
    }
}

pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// The text that shows after the first `cells` display columns are hidden.
pub fn skip_columns(text: &str, cells: usize) -> String {
    let mut used = 0usize;
    let mut out = String::new();
    for c in text.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > cells {
            out.push(c);
        }
        used += w;
    }
    out
}

pub fn truncate(text: &str, max_width: usize) -> String {
    if width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > max_width - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

pub fn truncate_start(text: &str, max_width: usize) -> String {
    if width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let mut tail = Vec::new();
    let mut used = 0;
    for c in text.chars().rev() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > max_width - 1 {
            break;
        }
        tail.push(c);
        used += w;
    }
    std::iter::once('…').chain(tail.into_iter().rev()).collect()
}

pub fn pad(text: &str, target: usize) -> String {
    let mut out = truncate(text, target);
    let w = width(&out);
    out.extend(std::iter::repeat_n(' ', target.saturating_sub(w)));
    out
}

pub fn pad_left(text: &str, target: usize) -> String {
    let out = truncate(text, target);
    let w = width(&out);
    let mut padded: String = std::iter::repeat_n(' ', target.saturating_sub(w)).collect();
    padded.push_str(&out);
    padded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn durations_are_compact_and_show_seconds() {
        assert_eq!(duration_short(0), "0s");
        assert_eq!(duration_short(42_999), "42s");
        assert_eq!(duration_short(65_000), "1m05s");
        assert_eq!(duration_short(3_599_000), "59m59s");
        assert_eq!(duration_short(3_720_000), "1h02m");
        assert_eq!(duration_short(101 * 3_600_000), "4d05h");
    }

    #[test]
    fn slugs_drop_stopwords_and_accents() {
        assert_eq!(slugify("Fix the login bug in the API", 3, 28), "fix-login-bug");
        assert_eq!(slugify("Corrigir a navegação do app", 3, 28), "corrigir-navegacao-app");
        assert_eq!(slugify("   ", 3, 28), "");
        assert_eq!(slugify("supercalifragilistic expialidocious", 3, 10), "supercalif");
        assert_eq!(slugify("open simulator projects now", 3, 15), "open-simulator");
    }

    #[test]
    fn paths_are_abbreviated_with_tilde() {
        let home = PathBuf::from("/home/me");
        assert_eq!(display_path(Path::new("/home/me/GitHub/x"), Some(&home)), "~/GitHub/x");
        assert_eq!(display_path(Path::new("/home/me"), Some(&home)), "~");
        assert_eq!(display_path(Path::new("/home/meow"), Some(&home)), "/home/meow");
        assert_eq!(display_path(Path::new("/srv"), None), "/srv");
    }

    #[test]
    fn project_label_uses_last_two_components() {
        assert_eq!(project_label(Path::new("/home/me/vibing/earth")), "vibing/earth");
        assert_eq!(project_label(Path::new("/earth")), "earth");
    }

    #[test]
    fn truncation_respects_display_width() {
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("hi", 5), "hi");
        assert_eq!(truncate("漢字漢字", 5), "漢字…");
        assert_eq!(width(&truncate("漢字漢字", 5)), 5);
        assert_eq!(pad("ab", 4), "ab  ");
        assert_eq!(pad_left("ab", 4), "  ab");
        assert_eq!(pad("abcdef", 4), "abc…");
    }

    #[test]
    fn start_truncation_keeps_the_end_of_paths() {
        assert_eq!(truncate_start("~/projects/studio/", 10), "…s/studio/");
        assert_eq!(width(&truncate_start("~/projects/studio/", 10)), 10);
        assert_eq!(truncate_start("short", 10), "short");
        assert_eq!(truncate_start("漢字漢字", 5), "…漢字");
        assert_eq!(truncate_start("abc", 0), "");
    }
}
