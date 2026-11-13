use crate::hooks::{self, Ask};
use crate::model::AgentKind;
use crate::orchestrator::{MAX_CONTEXT_ROWS, MAX_QUESTION_CHARS, MAX_ROW_CHARS, Mode, ToolAsk, cut};

const FRAME: &[char] = &['│', '┃', '|', '╭', '╮', '╰', '╯', '┌', '┐', '└', '┘', '─', '━', '═'];
const POINTERS: &[char] = &['❯', '›', '▶', '●', '>', '*'];
const OPTION_MARKS: &[char] = &['.', ')'];
const FIRST_OPTION: u32 = 1;
const CURSORS: &[char] = &['❯', '›', '▶', '●'];
const HINT_SEPARATOR: char = '·';
const KEY_HINTS: [&str; 6] = ["to confirm", "to cancel", "to select", "to navigate", "shift+tab", "ctrl+"];
const ASKS: char = '?';
const MAX_ROWS_TO_A_QUESTION: usize = 12;
const MAX_TAIL_CHARS: usize = 12;

pub fn context_rows(screen: &str) -> Vec<String> {
    let mut rows: Vec<String> = screen.lines().map(|row| cut(row.trim_end(), MAX_ROW_CHARS)).collect();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    let last = rows.split_off(rows.len().saturating_sub(MAX_CONTEXT_ROWS));
    last.into_iter().skip_while(String::is_empty).collect()
}

fn unframed(row: &str) -> &str {
    row.trim().trim_matches(FRAME).trim()
}

fn option_number(row: &str) -> Option<u32> {
    let item = unframed(row).trim_start_matches(POINTERS).trim_start();
    let digits = item.chars().take_while(char::is_ascii_digit).count();
    let rest = item[digits..].strip_prefix(OPTION_MARKS)?;
    (rest.starts_with(' ') && !rest.trim().is_empty()).then(|| item[..digits].parse().ok()).flatten()
}

fn is_key_hint(row: &str) -> bool {
    let row = row.to_lowercase();
    row.split(HINT_SEPARATOR).all(|part| KEY_HINTS.iter().any(|hint| part.contains(hint)))
}

fn menu_start(rows: &[String]) -> Option<usize> {
    let numbered = rows.iter().rposition(|row| option_number(row) == Some(FIRST_OPTION));
    numbered.or_else(|| rows.iter().rposition(|row| unframed(row).starts_with(CURSORS)))
}

fn asked_part(row: &str) -> Option<&str> {
    let end = row.rfind(ASKS)? + ASKS.len_utf8();
    let tail = row[end..].trim();
    Some(if tail.chars().count() <= MAX_TAIL_CHARS { row } else { &row[..end] })
}

fn asked_on_screen(rows: &[String]) -> Option<&str> {
    let above = &rows[..menu_start(rows).unwrap_or(rows.len())];
    let prose = |row: &&str| !row.is_empty() && option_number(row).is_none() && !is_key_hint(row);
    let mut candidates = above.iter().rev().map(|row| unframed(row)).filter(prose);
    let nearest = candidates.next()?;
    let asked = std::iter::once(nearest).chain(candidates).take(MAX_ROWS_TO_A_QUESTION).find_map(asked_part);
    Some(asked.unwrap_or(nearest))
}

pub fn question(ask: Option<&Ask>, rows: &[String]) -> String {
    let asked = ask.and_then(|ask| ask.detail.lines().map(str::trim).find(|line| !line.is_empty()));
    cut(asked.or_else(|| asked_on_screen(rows)).unwrap_or_default(), MAX_QUESTION_CHARS)
}

pub fn fresh_ask(ask: Option<Ask>, kind: AgentKind, since: u64) -> Option<Ask> {
    ask.filter(|ask| kind == AgentKind::Claude && ask.at_ms >= since)
}

pub fn tool_of(ask: &Ask) -> ToolAsk {
    ToolAsk { name: ask.tool.clone(), detail: ask.detail.clone() }
}

pub fn allowed_by_rule(mode: Mode, kind: AgentKind, ask: Option<&Ask>) -> bool {
    mode == Mode::Bypass && kind == AgentKind::Claude && ask.is_some_and(|ask| !hooks::is_question_tool(&ask.tool))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_string()).collect()
    }

    fn ask(tool: &str, detail: &str, at_ms: u64) -> Ask {
        Ask { tool: tool.into(), detail: detail.into(), at_ms }
    }

    #[test]
    fn the_context_is_the_last_rows_of_the_screen_without_the_blank_ones_below() {
        assert_eq!(context_rows("one  \n\ntwo\n   \n\n"), ["one", "", "two"]);
        assert!(context_rows("\n \n").is_empty());
        assert_eq!(context_rows("\n  \n\nasked\n\nhere\n"), ["asked", "", "here"], "nor the blank ones above");
        let tall: Vec<String> = (0..MAX_CONTEXT_ROWS + 5).map(|row| format!("row {row}")).collect();
        let kept = context_rows(&format!("{}\n\n\n", tall.join("\n")));
        assert_eq!(kept.len(), MAX_CONTEXT_ROWS);
        assert_eq!((kept[0].as_str(), kept[MAX_CONTEXT_ROWS - 1].as_str()), ("row 5", "row 44"));
        let wide = context_rows(&"ç".repeat(MAX_ROW_CHARS + 30));
        assert_eq!(wide[0].chars().count(), MAX_ROW_CHARS, "a wide row is cut by characters");
    }

    #[test]
    fn the_question_is_the_first_line_of_what_the_hook_reported() {
        let screen = rows(&["Do you want to proceed?", "❯ 1. Yes", "  2. No"]);
        assert_eq!(question(Some(&ask("Bash", "npm test\nRun the suite", 1)), &screen), "npm test");
        assert_eq!(
            question(Some(&ask("AskUserQuestion", "\n  Which database?\n  1. Postgres", 1)), &screen),
            "Which database?"
        );
        assert_eq!(
            question(Some(&ask("Bash", "  \n", 1)), &screen),
            "Do you want to proceed?",
            "an empty detail says nothing"
        );
        let long = "x".repeat(MAX_QUESTION_CHARS + 50);
        assert_eq!(question(Some(&ask("Bash", &long, 1)), &[]).chars().count(), MAX_QUESTION_CHARS);
    }

    #[test]
    fn without_a_hook_the_question_is_the_line_above_the_options_on_the_screen() {
        let claude = rows(&[
            "╭──────────────────────────────────────────────╮",
            "│ Edit file                                    │",
            "│ 410 + document.body.dataset.view = view;     │",
            "│ Do you want to make this edit to app.js?     │",
            "│ ❯ 1. Yes                                     │",
            "│   2. Yes, allow all edits during this session │",
            "│   3. No, and tell Claude what to do differently │",
            "╰──────────────────────────────────────────────╯",
        ]);
        assert_eq!(question(None, &claude), "Do you want to make this edit to app.js?");
        let codex = rows(&[
            " 1. an old list",
            " 2. printed before",
            " Would you like to make the following edits?",
            "",
            " › 1. Yes, proceed",
            "   2. No, and tell Codex what to do",
            "   Press enter to confirm or esc to cancel",
        ]);
        assert_eq!(question(None, &codex), "Would you like to make the following edits?");
        let numbered = rows(&["Pick one", "1) first", "2) second"]);
        assert_eq!(question(None, &numbered), "Pick one");
    }

    #[test]
    fn a_screen_without_options_gives_its_last_line_and_an_empty_one_gives_nothing() {
        assert_eq!(
            question(None, &rows(&["building…", "", "Overwrite the file? (y/n)", "───────"])),
            "Overwrite the file? (y/n)"
        );
        assert_eq!(question(None, &rows(&["version 1.2 is out"])), "version 1.2 is out", "a number is not an option");
        assert_eq!(question(None, &rows(&["❯ 1. Yes", "  2. No"])), "", "options with nothing above ask nothing");
        assert_eq!(question(None, &[]), "");
    }

    #[test]
    fn the_folder_trust_prompt_of_claude_asks_its_question_and_not_its_key_hints() {
        let trust = rows(&[
            " Accessing workspace:",
            "",
            r" C:\o\vh\proj\demo",
            "",
            " Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source",
            " project, or work from your team). If not, take a moment to review what's in this folder first.",
            "",
            " Claude Code'll be able to read, edit, and execute files here.",
            "",
            " Security guide",
            "",
            " ❯ No, exit",
            "   Yes, I trust this folder",
            "",
            " Enter to confirm · Esc to cancel",
        ]);
        assert_eq!(question(None, &trust), "Quick safety check: Is this a project you created or one you trust?");
        let second: Vec<String> =
            trust.iter().map(|row| row.replace(" ❯ No", "   No").replace("   Yes", " ❯ Yes")).collect();
        assert_eq!(
            question(None, &second),
            "Quick safety check: Is this a project you created or one you trust?",
            "whichever option the cursor is on"
        );
    }

    #[test]
    fn a_numbered_approval_menu_and_a_plain_yes_or_no_give_the_line_that_asks() {
        let approval = rows(&[
            "  $ cargo test --workspace",
            "",
            "  Allow command?",
            "  This runs in /w/app",
            "",
            "  › 1. Yes",
            "    2. Yes, and don't ask again this session",
            "    3. No, and tell Codex what to do differently",
            "",
            "  ↑↓ to navigate · enter to select · esc to cancel",
        ]);
        assert_eq!(question(None, &approval), "Allow command?", "the question wins over the line next to the menu");
        let plain = rows(&["Applying the migration", "Continue? (y/n)"]);
        assert_eq!(question(None, &plain), "Continue? (y/n)", "a short choice after the question stays with it");
        let hinted = rows(&["Continue? (y/n)", "", "Enter to confirm · Esc to cancel"]);
        assert_eq!(question(None, &hinted), "Continue? (y/n)");
        let stated = rows(&["Run this command", "  rm -rf build", "❯ 1. Yes", "  2. No"]);
        assert_eq!(question(None, &stated), "rm -rf build", "without a question the nearest line is the best there is");
        let mut far = vec!["what does this do?".to_string()];
        far.extend((0..MAX_ROWS_TO_A_QUESTION).map(|row| format!("log line {row}")));
        far.push("Allow the edit".into());
        assert_eq!(question(None, &far), "Allow the edit", "a question far up the screen is not what is asked now");
        assert_eq!(question(None, &rows(&["Enter to confirm · Esc to cancel"])), "", "hints alone ask nothing");
    }

    #[test]
    fn key_hints_are_told_from_what_the_agent_says() {
        for hint in [
            "Enter to confirm · Esc to cancel",
            "↑↓ to navigate · enter to select · esc to cancel",
            "Press enter to confirm or esc to cancel",
            "shift+tab to cycle",
            "ctrl+c to stop · ctrl+o to expand",
        ] {
            assert!(is_key_hint(hint), "{hint}");
        }
        for said in ["Do you want to proceed?", "Yes, I trust this folder", "Security guide", "Pick one · or two"] {
            assert!(!is_key_hint(said), "{said}");
        }
        assert_eq!(asked_part("One. Two? Three follows here, at length."), Some("One. Two?"));
        assert_eq!(asked_part("Overwrite? [y/N]"), Some("Overwrite? [y/N]"));
        assert_eq!(asked_part("no question"), None);
    }

    #[test]
    fn only_a_claude_ask_written_for_this_pause_is_used() {
        let written = |at_ms| Some(ask("Bash", "ls", at_ms));
        assert_eq!(fresh_ask(written(500), AgentKind::Claude, 500), written(500));
        assert_eq!(fresh_ask(written(650), AgentKind::Claude, 500), written(650), "the hook may land after the screen");
        assert_eq!(fresh_ask(written(499), AgentKind::Claude, 500), None, "an ask from an earlier pause is stale");
        assert_eq!(fresh_ask(written(500), AgentKind::Codex, 500), None, "other agents write no ask");
        assert_eq!(fresh_ask(None, AgentKind::Claude, 500), None);
        assert_eq!(tool_of(&ask("Bash", "ls", 1)), ToolAsk { name: "Bash".into(), detail: "ls".into() });
    }

    #[test]
    fn only_bypass_allows_a_claude_permission_request_without_asking_the_engine() {
        let permission = ask("Bash", "rm -rf build", 1);
        assert!(allowed_by_rule(Mode::Bypass, AgentKind::Claude, Some(&permission)));
        assert!(!allowed_by_rule(Mode::Moderate, AgentKind::Claude, Some(&permission)));
        assert!(!allowed_by_rule(Mode::Observe, AgentKind::Claude, Some(&permission)));
        assert!(!allowed_by_rule(Mode::Bypass, AgentKind::Codex, Some(&permission)), "only Claude has the hook");
        assert!(!allowed_by_rule(Mode::Bypass, AgentKind::Claude, None), "a pause seen only on the screen is judged");
        for tool in ["AskUserQuestion", "ExitPlanMode"] {
            let asked = ask(tool, "Which one?", 1);
            assert!(
                !allowed_by_rule(Mode::Bypass, AgentKind::Claude, Some(&asked)),
                "{tool} is a question, not a permission"
            );
        }
    }
}
