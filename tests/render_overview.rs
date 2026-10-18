use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

use orkai::config::Config;
use orkai::launch::Launcher;
use orkai::model::{
    AgentId, AgentKind, AgentRecord, AgentState, ExitInfo, NestedAgent, NewAgent, StatusReport, Worktree,
};
use orkai::paths::Home;
use orkai::protocol::HolderMsg;
use orkai::store::Store;
use orkai::subagents::Subagent;
use orkai::tui::app::{App, AppEvent, Confirm, Link, Prompt};
use orkai::tui::{render, theme};
use orkai::usage::{Limits, Problem, Usage, Window};
use orkai::util::now_ms;
use orkai::worktree::Changes;

const WIDTH: u16 = 150;
const HEIGHT: u16 = 40;
static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Scenario {
    app: App,
    events: mpsc::Receiver<AppEvent>,
    dir: PathBuf,
}

impl Drop for Scenario {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn scenario() -> Scenario {
    let dir = std::env::temp_dir().join(format!(
        "orkai-render-{}-{}-{}",
        std::process::id(),
        now_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Store::open(Home::at(dir.join("home"))).unwrap();
    store.create_session("session-2").unwrap();
    store.create_session("session-3").unwrap();
    let agents = [
        (
            "acme/web",
            "checkout-redesign",
            AgentKind::Claude,
            AgentState::Done,
            "Running design update and committing changes",
        ),
        (
            "acme/mobile",
            "simulator-builds",
            AgentKind::Claude,
            AgentState::Done,
            "Building and launching both apps in simulators",
        ),
        ("studio/game", "physics-merge", AgentKind::Claude, AgentState::Working, "Merging the new physics engine"),
        ("studio/game", "localhost-server", AgentKind::Terminal, AgentState::Idle, "npm run dev"),
        ("studio/assets", "term-3", AgentKind::Terminal, AgentState::Working, "node server.js"),
        (
            "studio/assets",
            "npm-start-error",
            AgentKind::Codex,
            AgentState::Idle,
            "Killing stale server process and restarting npm start",
        ),
        ("studio/assets", "sidebar-sessions", AgentKind::Claude, AgentState::Paused, "Session side panel"),
        (
            "lab/infra",
            "container-ssh-access",
            AgentKind::Claude,
            AgentState::Idle,
            "Retrieving container SSH credentials",
        ),
        ("orkai/cli", "folder-icon-update", AgentKind::Claude, AgentState::Done, "Folder icon update"),
        (
            "orkai/cli",
            "docker-install",
            AgentKind::Claude,
            AgentState::Working,
            "Installing Docker inside the container",
        ),
    ];
    let mut ids = Vec::new();
    for (project, name, kind, _, _) in &agents {
        let cwd = dir.join(project);
        std::fs::create_dir_all(&cwd).unwrap();
        let record = store
            .create_agent(NewAgent {
                session: "default".into(),
                name: (*name).into(),
                kind: *kind,
                cwd,
                task: String::new(),
            })
            .unwrap();
        ids.push(record.id);
    }
    let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
    let (tx, events) = mpsc::channel();
    let mut app = App::new(launcher, tx, Rect::new(0, 0, WIDTH, HEIGHT)).unwrap();
    let now = now_ms();
    for (index, (id, (_, _, _, state, activity))) in ids.iter().zip(agents.iter()).enumerate() {
        let entry = app.agents.get_mut(id).unwrap();
        let offset = (index as u64 + 1) * 97_000;
        entry.link = Link::Connecting(0);
        entry.status = Some(StatusReport {
            state: *state,
            state_since_ms: now - offset / 3,
            turn_started_ms: (*state == AgentState::Working).then_some(now - offset / 2),
            last_turn_ms: matches!(state, AgentState::Done | AgentState::Idle).then_some(60_000),
            started_at_ms: now - offset * 9,
            title: String::new(),
            activity: (*activity).into(),
            rows: 24,
            cols: 80,
            exit: None,
            nested: None,
        });
    }
    app.where_dir = dir.join("studio/game");
    app.hostname = "workstation".into();
    app.selected = Some(ids[4]);
    Scenario { app, events, dir }
}

fn render(app: &mut App) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    terminal.draw(|frame| render::draw(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn color_json(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("\"#{r:02x}{g:02x}{b:02x}\""),
        Color::Reset => "null".into(),
        other => format!("\"{other:?}\""),
    }
}

fn dump(buffer: &Buffer, path: &str) {
    let mut cells = Vec::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            cells.push(format!(
                "[{},{},{},{}]",
                serde_json::to_string(cell.symbol()).unwrap(),
                color_json(cell.fg),
                color_json(cell.bg),
                cell.modifier.contains(Modifier::BOLD)
            ));
        }
    }
    let json = format!(
        "{{\"width\":{},\"height\":{},\"cells\":[{}]}}",
        buffer.area.width,
        buffer.area.height,
        cells.join(",")
    );
    std::fs::write(path, json).unwrap();
}

#[test]
fn overview_shows_sessions_stats_projects_and_composer() {
    let mut scenario = scenario();
    let now = now_ms();
    let window =
        |used_percent, minutes: u64| Some(Window { used_percent, resets_at_ms: Some(now + minutes * 60_000 + 30_000) });
    let usage = |session, weekly| Usage {
        limits: Some(Limits { session, weekly }),
        problem: None,
        updated_at_ms: Some(now - 34_000),
    };
    scenario.app.usage.insert(AgentKind::Claude, usage(window(62.0, 134), window(23.0, 6_060)));
    scenario.app.usage.insert(AgentKind::Codex, usage(window(12.0, 221), window(76.0, 3_480)));
    let buffer = render(&mut scenario.app);
    if let Ok(path) = std::env::var("ORKAI_RENDER_DUMP") {
        dump(&buffer, &path);
    }
    let screen = text(&buffer);
    for expected in [
        "› default",
        "session-2",
        "+ new session",
        "agents",
        "working",
        "paused",
        "done",
        "acme/web",
        "studio/assets",
        "checkout-redesign",
        "localhost-server",
        "Running design update",
        "write a task",
        "who claude",
        "where",
        "usage · updated 34s ago",
        "62%  resets 2h14m",
        "23%  resets 4d05h",
        "12%  resets 3h41m",
        "76%  resets 58h00m",
    ] {
        assert!(screen.contains(expected), "missing {expected:?} in:\n{screen}");
    }
    assert!(screen.contains("  8 agents") || screen.contains("   8 "), "{screen}");
    assert!(screen.contains("▸"), "selection marker missing");
}

#[test]
fn stats_count_only_live_ai_agents() {
    let scenario = scenario();
    let stats = scenario.app.stats();
    assert_eq!(stats.agents, 8);
    assert_eq!(stats.working, 2);
    assert_eq!(stats.paused, 1);
    assert_eq!(stats.done, 3);
}

#[test]
fn agents_are_grouped_by_project_in_creation_order() {
    let scenario = scenario();
    let labels: Vec<String> = scenario.app.groups().into_iter().map(|g| g.label).collect();
    assert_eq!(labels, vec!["acme/web", "acme/mobile", "studio/game", "studio/assets", "lab/infra", "orkai/cli"]);
}

#[test]
fn help_overlay_and_small_screens_render() {
    let mut scenario = scenario();
    scenario.app.show_help = true;
    let screen = text(&render(&mut scenario.app));
    assert!(screen.contains("orkai · keys"));
    scenario.app.screen = Rect::new(0, 0, 30, 8);
    let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
    terminal.draw(|frame| render::draw(frame, &mut scenario.app)).unwrap();
}

#[test]
fn failed_recovery_returns_agent_to_interrupted() {
    let mut scenario = scenario();
    let id = *scenario.app.agents.keys().next().unwrap();
    scenario.app.handle(orkai::tui::app::AppEvent::Restarted { id, result: Err("directory missing".into()) });
    assert_eq!(scenario.app.agents[&id].state(), AgentState::Lost);
    let notice = scenario.app.notice.as_ref().unwrap();
    assert!(notice.text.contains("could not recover"), "{}", notice.text);
}

#[test]
fn agent_view_keeps_navigation_hint_with_long_paths() {
    let mut scenario = scenario();
    let id = *scenario.app.agents.keys().next().unwrap();
    let long =
        std::path::PathBuf::from("/private/var/folders/36/tjdph2t965j8snz9_vkdnw0r0000gn/T/a/very/long/project/path");
    scenario.app.agents.get_mut(&id).unwrap().record.cwd = long;
    scenario.app.view = orkai::tui::app::View::Agent(id);
    let screen = text(&render(&mut scenario.app));
    let status = screen.lines().last().unwrap();
    assert!(status.contains("ctrl+] overview"), "{status}");
    assert!(status.contains('…'), "{status}");
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle(AppEvent::Input(Event::Key(KeyEvent::new(code, modifiers))));
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c), KeyModifiers::NONE);
    }
}

#[test]
fn where_command_suggests_completes_and_cycles_folders() {
    let mut scenario = scenario();
    let root = scenario.dir.clone();
    scenario.app.where_dir = root.clone();
    let app = &mut scenario.app;

    type_text(app, "/wh");
    press(app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.task.text(), "/wh");
    type_text(app, "e");
    press(app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.task.text(), "/where ");
    assert!(app.paths.is_active());
    assert_eq!(app.paths.listing(), Some(root.as_path()));
    assert!(app.paths.suggestions().contains(&"studio".to_string()));
    assert!(app.paths.suggestions().contains(&"acme".to_string()));

    type_text(app, "st");
    assert_eq!(app.paths.suggestions(), ["studio"]);
    press(app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.task.text(), "/where studio/");
    assert_eq!(app.paths.suggestions(), ["assets", "game"]);

    let buffer = render(app);
    if let Ok(path) = std::env::var("ORKAI_RENDER_DUMP_WHERE") {
        dump(&buffer, &path);
    }
    let screen = text(&buffer);
    let hints = screen.lines().last().unwrap();
    assert!(hints.contains("assets/") && hints.contains("game/"), "{hints}");

    press(app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.task.text(), "/where studio/assets/");
    assert_eq!(app.paths.selected(), Some(0));
    press(app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.task.text(), "/where studio/game/");
    press(app, KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(app.task.text(), "/where studio/assets/");
    press(app, KeyCode::Tab, KeyModifiers::NONE);

    press(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.where_dir.ends_with("studio/game"), "{}", app.where_dir.display());
    assert!(app.task.is_empty());
    assert!(!app.paths.is_active());
    assert!(app.notice.as_ref().unwrap().text.contains("new tasks in default"));
}

#[test]
fn where_is_remembered_per_session() {
    let mut scenario = scenario();
    let root = scenario.dir.clone();
    scenario.app.where_dir = root.clone();
    let app = &mut scenario.app;
    app.run_command("/where studio/game");
    app.run_command("/session session-2");
    assert!(app.where_dir.ends_with("studio/game"));
    app.run_command(&format!("/where {}", root.join("acme").join("web").display()));
    assert!(app.where_dir.ends_with("acme/web"));
    app.run_command("/session default");
    assert!(app.where_dir.ends_with("studio/game"), "{}", app.where_dir.display());
    app.run_command("/session session-2");
    assert!(app.where_dir.ends_with("acme/web"));
}

#[test]
fn where_prompt_lists_and_completes_folders() {
    let mut scenario = scenario();
    let root = scenario.dir.clone();
    scenario.app.where_dir = root.join("studio");
    let app = &mut scenario.app;
    press(app, KeyCode::Char('l'), KeyModifiers::CONTROL);
    assert!(matches!(app.prompt, Some(Prompt::Where(_))));
    assert_eq!(app.paths.suggestions(), ["assets", "game"]);
    type_text(app, "g");
    assert_eq!(app.paths.suggestions(), ["game"]);
    press(app, KeyCode::Tab, KeyModifiers::NONE);
    press(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.prompt.is_none());
    assert!(app.where_dir.ends_with("studio/game"), "{}", app.where_dir.display());
}

#[test]
fn unknown_folder_reports_error_and_keeps_where() {
    let mut scenario = scenario();
    let before = scenario.app.where_dir.clone();
    scenario.app.run_command("/where does-not-exist");
    assert_eq!(scenario.app.where_dir, before);
    assert!(scenario.app.notice.as_ref().unwrap().text.contains("is not a directory"));
}

fn without_agent_programs(app: &mut App) {
    let config = Config {
        claude_command: "orkai-missing-claude".into(),
        codex_command: "orkai-missing-codex".into(),
        ..Config::default()
    };
    let store = app.launcher.store().clone();
    app.launcher = Launcher::new(store, config, &std::env::current_exe().unwrap()).unwrap();
}

fn write_skill(dir: &std::path::Path, name: &str) {
    let file = dir.join(name).join("SKILL.md");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, format!("---\nname: {name}\ndescription: test skill\n---\n")).unwrap();
}

fn launched(scenario: &Scenario) -> Result<AgentRecord, String> {
    loop {
        if let AppEvent::Launched { result } = scenario.events.recv_timeout(Duration::from_secs(20)).unwrap() {
            return result;
        }
    }
}

#[test]
fn slash_suggests_completes_and_launches_agent_skills() {
    let mut scenario = scenario();
    write_skill(&scenario.app.where_dir.join(".claude").join("skills"), "orkai-render-skill");
    without_agent_programs(&mut scenario.app);
    let app = &mut scenario.app;

    type_text(app, "/orkai-ren");
    let screen = text(&render(app));
    let hints = screen.lines().last().unwrap();
    assert!(hints.contains("/orkai-render-skill"), "{hints}");
    press(app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.task.text(), "/orkai-render-skill ");
    type_text(app, "now");
    press(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.task.is_empty());
    assert_eq!(app.launching, 1);
    assert!(app.notice.as_ref().unwrap().text.contains("starting claude orkai-render-skill"));
    let error = launched(&scenario).unwrap_err();
    assert!(error.contains("orkai-missing-claude"), "{error}");
}

#[test]
fn codex_skills_are_suggested_only_for_codex() {
    let mut scenario = scenario();
    write_skill(&scenario.app.where_dir.join(".agents").join("skills"), "orkai-codex-skill");
    without_agent_programs(&mut scenario.app);
    let app = &mut scenario.app;

    type_text(app, "/orkai-codex");
    assert!(!text(&render(app)).lines().last().unwrap().contains("/orkai-codex-skill"));
    press(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.notice.as_ref().unwrap().text.contains("unknown command /orkai-codex"));

    app.who = AgentKind::Codex;
    type_text(app, "/orkai-codex");
    assert!(text(&render(app)).lines().last().unwrap().contains("/orkai-codex-skill"));
    press(app, KeyCode::Tab, KeyModifiers::NONE);
    press(app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.launching, 1);
    assert!(app.notice.as_ref().unwrap().text.contains("starting codex"));
    assert!(launched(&scenario).unwrap_err().contains("orkai-missing-codex"));
}

#[test]
fn delete_key_confirms_in_a_popup_then_removes_the_agent() {
    let mut scenario = scenario();
    let app = &mut scenario.app;
    let id = app.selected.unwrap();
    let name = app.agents[&id].record.name.clone();

    press(app, KeyCode::Delete, KeyModifiers::NONE);
    assert!(matches!(app.prompt, Some(Prompt::Confirm(Confirm::Delete(target))) if target == id));
    let screen = text(&render(app));
    assert!(screen.contains("delete agent"), "{screen}");
    assert!(screen.contains(&format!("delete {name}?")), "{screen}");
    assert!(screen.contains("y / enter confirm"), "{screen}");

    press(app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(app.prompt.is_none());
    assert!(app.agents.contains_key(&id));

    press(app, KeyCode::Delete, KeyModifiers::NONE);
    press(app, KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(!app.agents.contains_key(&id));
    assert!(!app.launcher.store().home().agent_dir(id).exists());
    assert!(app.notice.as_ref().unwrap().text.contains(&format!("{name} removed")));
}

#[test]
fn delete_key_removes_stopped_agents_and_edits_text_while_typing() {
    let mut scenario = scenario();
    let app = &mut scenario.app;
    let id = app.selected.unwrap();
    app.agents.get_mut(&id).unwrap().link = Link::Exited(ExitInfo { code: Some(0), error: None, at_ms: now_ms() });

    type_text(app, "ab");
    press(app, KeyCode::Left, KeyModifiers::NONE);
    press(app, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(app.task.text(), "a");
    assert!(app.prompt.is_none());

    press(app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    press(app, KeyCode::Delete, KeyModifiers::NONE);
    assert!(matches!(app.prompt, Some(Prompt::Confirm(Confirm::Remove(target))) if target == id));
    assert!(text(&render(app)).contains("remove agent"));
    press(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!app.agents.contains_key(&id));
}

#[test]
fn ctrl_x_uses_the_popup_and_a_click_outside_cancels_it() {
    let mut scenario = scenario();
    let app = &mut scenario.app;
    let id = app.selected.unwrap();
    press(app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert!(matches!(app.prompt, Some(Prompt::Confirm(Confirm::Kill(target))) if target == id));
    let screen = text(&render(app));
    assert!(screen.contains("stop agent"), "{screen}");
    assert!(!screen.contains("[y/N]"), "{screen}");

    let click =
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 1, row: 1, modifiers: KeyModifiers::NONE };
    app.handle(AppEvent::Input(Event::Mouse(click)));
    assert!(app.prompt.is_none());
    assert!(app.agents.contains_key(&id));
    assert_eq!(app.notice.as_ref().unwrap().text, "cancelled");
}

#[test]
fn shift_enter_and_pasted_line_breaks_stay_in_the_task() {
    let mut scenario = scenario();
    let app = &mut scenario.app;
    type_text(app, "first line");
    press(app, KeyCode::Enter, KeyModifiers::SHIFT);
    type_text(app, "second line");
    app.handle(AppEvent::Input(Event::Paste("\r\nthird line".into())));
    assert_eq!(app.task.text(), "first line\nsecond line\nthird line");
    assert_eq!(app.launching, 0);

    let screen = text(&render(app));
    let lines: Vec<&str> = screen.lines().collect();
    let n = lines.len();
    assert!(lines[n - 5].contains("first line"), "{screen}");
    assert!(lines[n - 4].contains("second line"), "{screen}");
    assert!(lines[n - 3].contains("third line"), "{screen}");
    assert!(lines[n - 2].contains("who claude"), "{screen}");

    press(app, KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(app.task.line_count(), 4);
    assert_eq!(app.launching, 0);
}

#[test]
fn running_subagents_are_listed_under_their_agent_until_they_finish() {
    let mut scenario = scenario();
    let parent = scenario.app.agents.values().find(|e| e.record.name == "physics-merge").unwrap().record.id;
    let stopped = scenario.app.agents.values().find(|e| e.record.name == "docker-install").unwrap().record.id;
    scenario.app.agents.get_mut(&stopped).unwrap().link =
        Link::Exited(ExitInfo { code: Some(0), error: None, at_ms: now_ms() });
    let subagent = Subagent {
        id: "a1".into(),
        role: "Explore".into(),
        description: "Map the auth flow".into(),
        activity: "Read · src/auth.rs".into(),
        started_at_ms: now_ms() - 42_000,
    };
    let found = BTreeMap::from([(parent, vec![subagent.clone()]), (stopped, vec![subagent])]);
    scenario.app.handle(AppEvent::Subagents(found));
    assert!(scenario.app.agents[&stopped].subagents.is_empty());

    let screen = text(&render(&mut scenario.app));
    let lines: Vec<&str> = screen.lines().collect();
    let row = lines.iter().position(|l| l.contains("physics-merge")).unwrap();
    let sub = lines[row + 1];
    for expected in ["↳", "Explore", "42s", "subagent", "Map the auth flow · Read · src/auth.rs"] {
        assert!(sub.contains(expected), "missing {expected:?} in {sub:?}");
    }
    assert_eq!(screen.matches("Explore").count(), 1, "{screen}");

    scenario.app.handle(AppEvent::Subagents(BTreeMap::new()));
    let screen = text(&render(&mut scenario.app));
    assert!(!screen.contains("Explore"), "{screen}");
}

#[test]
fn agents_waiting_for_a_decision_are_highlighted_in_yellow() {
    let mut scenario = scenario();
    let buffer = render(&mut scenario.app);
    let screen = text(&buffer);
    let (row, line) = screen.lines().enumerate().find(|(_, l)| l.contains("sidebar-sessions")).unwrap();
    assert!(line.contains("waiting for you · Session side panel"), "{line}");
    assert!(line.contains('‖'), "{line}");
    let byte = line.find("sidebar-sessions").unwrap();
    let column = u16::try_from(line[..byte].chars().count()).unwrap();
    assert_eq!(buffer[(column, u16::try_from(row).unwrap())].fg, theme::YELLOW);
}

fn line_with<'a>(screen: &'a str, needle: &str) -> (usize, &'a str) {
    screen.lines().enumerate().find(|(_, l)| l.contains(needle)).unwrap_or_else(|| {
        panic!(
            "{needle:?} in:
{screen}"
        )
    })
}

fn column_of(line: &str, needle: &str) -> u16 {
    u16::try_from(line[..line.find(needle).unwrap()].chars().count()).unwrap()
}

#[test]
fn usage_panel_sits_right_of_the_stats_and_counts_down_to_the_reset() {
    let mut scenario = scenario();
    let now = now_ms();
    let claude = Limits {
        session: Some(Window { used_percent: 55.0, resets_at_ms: Some(now + 3 * 3_600_000 + 30_000) }),
        weekly: Some(Window { used_percent: 92.0, resets_at_ms: Some(now + 33 * 3_600_000 + 30_000) }),
    };
    let codex = Limits { session: None, weekly: Some(Window { used_percent: 75.0, resets_at_ms: None }) };
    scenario.app.handle(AppEvent::Usage { kind: AgentKind::Claude, result: Ok(claude) });
    scenario.app.handle(AppEvent::Usage { kind: AgentKind::Codex, result: Ok(codex) });
    scenario.app.handle(AppEvent::Usage { kind: AgentKind::Codex, result: Err(Problem::Unavailable) });
    let buffer = render(&mut scenario.app);
    let screen = text(&buffer);

    let (row, agents) = line_with(&screen, " agents ");
    for expected in ["claude", "5h", "55%", "resets 3h00m"] {
        assert!(agents.contains(expected), "missing {expected:?} in {agents:?}");
    }
    assert!(column_of(agents, "claude") > column_of(agents, "agents"), "{agents}");
    let working = screen.lines().nth(row + 1).unwrap();
    for expected in ["week", "92%", "resets 33h00m"] {
        assert!(working.contains(expected), "missing {expected:?} in {working:?}");
    }
    let paused = screen.lines().nth(row + 2).unwrap();
    assert!(paused.contains("paused") && paused.contains("codex"), "{paused}");
    assert!(paused.contains("n/a · unavailable"), "{paused}");
    let done = screen.lines().nth(row + 3).unwrap();
    assert!(done.contains("week") && done.contains("75%") && !done.contains("resets"), "{done}");
    let header = screen.lines().nth(row - 1).unwrap();
    assert!(header.contains("usage · updated 0s ago"), "{header}");

    let y = u16::try_from(row).unwrap();
    let percent = column_of(agents, "55%");
    assert_eq!(buffer[(percent, y)].fg, theme::GREEN);
    assert_eq!(buffer[(column_of(working, "92%"), y + 1)].fg, theme::RED);
    assert_eq!(buffer[(column_of(done, "75%"), y + 3)].fg, theme::YELLOW);
    let bar = column_of(agents, "━");
    assert_eq!(agents.matches('━').count(), 7, "{agents}");
    assert_eq!(buffer[(bar, y)].fg, theme::GREEN);
}

#[test]
fn usage_panel_explains_missing_logins_before_any_data() {
    let mut scenario = scenario();
    let screen = text(&render(&mut scenario.app));
    let (row, agents) = line_with(&screen, " agents ");
    assert!(agents.contains("claude") && agents.contains("checking…"), "{agents}");
    assert!(screen.lines().nth(row - 1).unwrap().contains("usage · checking…"), "{screen}");

    scenario.app.handle(AppEvent::Usage { kind: AgentKind::Claude, result: Err(Problem::Expired) });
    scenario.app.handle(AppEvent::Usage { kind: AgentKind::Codex, result: Err(Problem::SignedOut) });
    let screen = text(&render(&mut scenario.app));
    assert!(screen.lines().nth(row).unwrap().contains("claude  login expired"), "{screen}");
    assert!(screen.lines().nth(row + 2).unwrap().contains("codex   not signed in"), "{screen}");

    scenario.app.screen = Rect::new(0, 0, 60, 12);
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
    terminal.draw(|frame| render::draw(frame, &mut scenario.app)).unwrap();
}

#[test]
fn bang_runs_the_command_in_a_new_terminal_without_changing_who() {
    let mut scenario = scenario();
    let store = scenario.app.launcher.store().clone();
    let config = Config { shell: Some("orkai-missing-shell".into()), ..Config::default() };
    scenario.app.launcher = Launcher::new(store, config, &std::env::current_exe().unwrap()).unwrap();
    let app = &mut scenario.app;
    assert_eq!(app.who, AgentKind::Claude);

    type_text(app, "!");
    press(app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.launching, 0);
    assert!(app.notice.as_ref().unwrap().text.contains("type a command after !"));

    type_text(app, "npm run dev");
    let buffer = render(app);
    let screen = text(&buffer);
    let lines: Vec<&str> = screen.lines().collect();
    let n = lines.len();
    assert!(lines[n - 3].contains("!npm run dev"), "{screen}");
    assert!(lines[n - 1].contains("enter runs it in a new terminal · who stays claude"), "{screen}");
    let row = u16::try_from(n - 3).unwrap();
    assert_eq!(buffer[(column_of(lines[n - 3], "!"), row)].fg, theme::TERMINAL);

    press(app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.task.is_empty());
    assert_eq!(app.launching, 1);
    assert_eq!(app.who, AgentKind::Claude);
    assert!(
        app.notice.as_ref().unwrap().text.contains("starting terminal npm-run-dev"),
        "{:?}",
        app.notice.as_ref().unwrap().text
    );
    let error = launched(&scenario).unwrap_err();
    assert!(error.contains("orkai-missing-shell"), "{error}");
}

#[test]
fn terminals_running_an_agent_are_shown_counted_and_watched_as_that_agent() {
    let mut scenario = scenario();
    let now = now_ms();
    let id = scenario.app.agents.values().find(|e| e.record.name == "localhost-server").unwrap().record.id;
    let elsewhere = scenario.dir.join("elsewhere");
    let session = "5bd0ccb0-0d09-47ca-9524-6eb740341fa9";
    {
        let status = scenario.app.agents.get_mut(&id).unwrap().status.as_mut().unwrap();
        status.state = AgentState::Working;
        status.turn_started_ms = Some(now - 42_000);
        status.activity = "Fix login bug".into();
        status.nested = Some(NestedAgent {
            kind: AgentKind::Claude,
            pid: 4242,
            conversation_id: Some(session.into()),
            cwd: Some(elsewhere.clone()),
            since_ms: now - 50_000,
        });
    }
    let stats = scenario.app.stats();
    assert_eq!((stats.agents, stats.working), (9, 3));
    let entry = &scenario.app.agents[&id];
    assert_eq!(entry.kind(), AgentKind::Claude);
    let parent = entry.subagent_parent().unwrap();
    assert_eq!(parent.kind, AgentKind::Claude);
    assert_eq!(parent.conversation_id.as_deref(), Some(session));
    assert_eq!(parent.cwd, elsewhere);
    assert_eq!(parent.created_at_ms, now - 50_000);
    assert!(parent.busy);

    let subagent = Subagent {
        id: "a1".into(),
        role: "Explore".into(),
        description: "Map the login flow".into(),
        activity: "Read · src/login.rs".into(),
        started_at_ms: now - 7_000,
    };
    scenario.app.handle(AppEvent::Subagents(BTreeMap::from([(id, vec![subagent])])));
    let screen = text(&render(&mut scenario.app));
    let (row, line) = line_with(&screen, "localhost-server");
    assert!(line.contains("42s") && line.contains("claude   Fix login bug"), "{line}");
    assert!(!line.contains("terminal"), "{line}");
    let sub = screen.lines().nth(row + 1).unwrap();
    assert!(sub.contains("↳") && sub.contains("Explore") && sub.contains("Map the login flow"), "{sub}");

    scenario.app.agents.get_mut(&id).unwrap().status.as_mut().unwrap().nested = None;
    assert_eq!(scenario.app.agents[&id].kind(), AgentKind::Terminal);
    assert!(scenario.app.agents[&id].subagent_parent().is_none());
    assert_eq!(scenario.app.stats().agents, 8);
    let screen = text(&render(&mut scenario.app));
    assert!(line_with(&screen, "localhost-server").1.contains("terminal"), "{screen}");
}

fn agent_named(app: &App, name: &str) -> AgentId {
    app.agents.values().find(|e| e.record.name == name).unwrap().record.id
}

fn push_state(app: &mut App, id: AgentId, state: AgentState) {
    let mut status = app.agents[&id].status.clone().unwrap();
    status.state = state;
    status.state_since_ms = now_ms();
    app.handle(AppEvent::Holder { id, generation: 0, msg: Some(HolderMsg::Status(status)) });
}

#[test]
fn agents_that_need_you_raise_one_alert_and_show_in_the_window_title() {
    let mut scenario = scenario();
    let app = &mut scenario.app;
    assert_eq!(app.window_title(), "‖1 ✔3 · Orkai");
    assert!(app.take_alerts().is_empty());

    let working = agent_named(app, "physics-merge");
    push_state(app, working, AgentState::Paused);
    let alerts = app.take_alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].id, working);
    assert_eq!(alerts[0].state, AgentState::Paused);
    assert_eq!(alerts[0].kind, AgentKind::Claude);
    assert_eq!(alerts[0].activity, "Merging the new physics engine");
    assert_eq!(alerts[0].message(), "physics-merge is waiting for you");
    assert_eq!(app.notice.as_ref().unwrap().text, "physics-merge is waiting for you · alt+n opens it");
    assert_eq!(app.window_title(), "‖2 ✔3 · Orkai");

    push_state(app, working, AgentState::Working);
    push_state(app, working, AgentState::Paused);
    assert!(app.take_alerts().is_empty(), "a flicker back into the same state must not ring again");

    push_state(app, working, AgentState::Done);
    let alerts = app.take_alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].message(), "physics-merge finished");
    assert_eq!(app.notice.as_ref().unwrap().text, "physics-merge finished · alt+n opens it");

    let viewed = agent_named(app, "docker-install");
    app.view = orkai::tui::app::View::Agent(viewed);
    push_state(app, viewed, AgentState::Done);
    assert!(app.take_alerts().is_empty(), "the agent on screen must not ring");

    let fresh = agent_named(app, "container-ssh-access");
    let mut status = app.agents.get_mut(&fresh).unwrap().status.take().unwrap();
    status.state = AgentState::Paused;
    app.handle(AppEvent::Holder { id: fresh, generation: 0, msg: Some(HolderMsg::Status(status)) });
    assert!(app.take_alerts().is_empty(), "the first status after connecting is not a change");

    let terminal = agent_named(app, "term-3");
    push_state(app, terminal, AgentState::Idle);
    assert!(app.take_alerts().is_empty());

    for entry in app.agents.values_mut() {
        if let Some(status) = entry.status.as_mut() {
            status.state = AgentState::Idle;
        }
    }
    assert_eq!(app.window_title(), "Orkai");
}

#[test]
fn alt_n_opens_the_longest_waiting_agent_first_then_the_next_from_anywhere() {
    let mut scenario = scenario();
    let app = &mut scenario.app;
    let paused = agent_named(app, "sidebar-sessions");
    let oldest_done = agent_named(app, "folder-icon-update");
    let help = text(&{
        app.show_help = true;
        let buffer = render(app);
        app.show_help = false;
        buffer
    });
    assert!(help.contains("alt+n") && help.contains("next agent waiting for you"), "{help}");

    press(app, KeyCode::Char('n'), KeyModifiers::ALT);
    assert_eq!(app.view, orkai::tui::app::View::Agent(paused));
    assert!(app.task.is_empty());
    let status = text(&render(app)).lines().last().unwrap().to_string();
    assert!(status.contains("‖ 3 need you · alt+n · ctrl+] overview"), "{status}");

    press(app, KeyCode::Char('n'), KeyModifiers::ALT);
    assert_eq!(app.view, orkai::tui::app::View::Agent(oldest_done));
    assert_eq!(app.tabs, vec![paused, oldest_done]);

    press(app, KeyCode::Char(']'), KeyModifiers::CONTROL);
    for entry in app.agents.values_mut() {
        if let Some(status) = entry.status.as_mut() {
            status.state = AgentState::Idle;
        }
    }
    press(app, KeyCode::Char('n'), KeyModifiers::ALT);
    assert_eq!(app.view, orkai::tui::app::View::Overview);
    assert_eq!(app.notice.as_ref().unwrap().text, "no agent is waiting for you");
    let status = {
        app.view = orkai::tui::app::View::Agent(paused);
        let line = text(&render(app)).lines().last().unwrap().to_string();
        app.view = orkai::tui::app::View::Overview;
        line
    };
    assert!(!status.contains("need you"), "{status}");

    let elsewhere = agent_named(app, "localhost-server");
    {
        let entry = app.agents.get_mut(&paused).unwrap();
        entry.record.session = "session-2".into();
        entry.status.as_mut().unwrap().state = AgentState::Paused;
    }
    assert_ne!(elsewhere, paused);
    press(app, KeyCode::Char('n'), KeyModifiers::ALT);
    assert_eq!(app.session, "session-2");
    assert_eq!(app.view, orkai::tui::app::View::Agent(paused));
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_repo(dir: &std::path::Path) -> PathBuf {
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "orkai@example.com"]);
    git(&repo, &["config", "user.name", "orkai"]);
    git(&repo, &["config", "core.autocrlf", "false"]);
    std::fs::write(repo.join("readme.txt"), "hello\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    repo
}

fn next_event(scenario: &Scenario, matches: impl Fn(&AppEvent) -> bool) -> AppEvent {
    loop {
        let event = scenario.events.recv_timeout(Duration::from_secs(30)).unwrap();
        if matches(&event) {
            return event;
        }
    }
}

fn put_in_worktree(scenario: &mut Scenario, name: &str) -> (AgentId, Worktree) {
    let repo = git_repo(&scenario.dir);
    let id = agent_named(&scenario.app, name);
    let home = scenario.app.launcher.store().home().clone();
    let (worktree, cwd) = orkai::worktree::create(&home, id, name, &repo).unwrap();
    let entry = scenario.app.agents.get_mut(&id).unwrap();
    entry.record.cwd = cwd;
    entry.record.worktree = Some(worktree.clone());
    scenario.app.launcher.store().save_agent(&entry.record).unwrap();
    (id, worktree)
}

#[test]
fn the_worktree_switch_is_shown_clickable_and_remembered() {
    let mut scenario = scenario();
    let app = &mut scenario.app;
    let footer = |app: &mut App| text(&render(app)).lines().nth(HEIGHT as usize - 2).unwrap().to_string();
    assert!(footer(app).contains("who claude   worktree off   where"), "{}", footer(app));

    press(app, KeyCode::Char('t'), KeyModifiers::CONTROL);
    assert!(app.worktree);
    assert!(footer(app).contains("worktree on"));
    assert!(app.notice.as_ref().unwrap().text.contains("own git worktree"));
    assert!(app.launcher.store().load_ui().worktree);

    let line = footer(app);
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: column_of(&line, "worktree"),
        row: HEIGHT - 2,
        modifiers: KeyModifiers::NONE,
    };
    app.handle(AppEvent::Input(Event::Mouse(click)));
    assert!(!app.worktree);
    app.run_command("/worktree on");
    assert!(app.worktree);
    app.run_command("/worktree off");
    assert!(!app.worktree);
    app.run_command("/worktree maybe");
    assert_eq!(app.notice.as_ref().unwrap().text, "usage: /worktree [on|off]");
    app.run_command("/wt");
    assert!(app.worktree);
}

#[test]
fn agents_in_a_worktree_show_their_branch_and_changes_under_their_project() {
    let mut scenario = scenario();
    let (id, worktree) = put_in_worktree(&mut scenario, "physics-merge");
    let labels: Vec<String> = scenario.app.groups().into_iter().map(|g| g.label).collect();
    assert!(labels.iter().any(|l| l.ends_with("/repo")), "{labels:?}");
    assert!(!labels.iter().any(|l| l.contains("worktrees")), "{labels:?}");

    let screen = text(&render(&mut scenario.app));
    assert!(line_with(&screen, "physics-merge").1.contains("claude   ⎇ …  Merging"), "{screen}");

    let changes = Changes { files: 4, insertions: 12, deletions: 3 };
    scenario.app.handle(AppEvent::Changes(BTreeMap::from([(id, changes)])));
    let buffer = render(&mut scenario.app);
    let screen = text(&buffer);
    let (row, line) = line_with(&screen, "physics-merge");
    assert!(line.contains("⎇ 4 files +12 −3  Merging the new physics engine"), "{line}");
    let y = u16::try_from(row).unwrap();
    assert_eq!(buffer[(column_of(line, "+12"), y)].fg, theme::GREEN);
    assert_eq!(buffer[(column_of(line, "−3"), y)].fg, theme::RED);

    scenario.app.handle(AppEvent::Changes(BTreeMap::from([(id, Changes::default())])));
    assert!(line_with(&text(&render(&mut scenario.app)), "physics-merge").1.contains("⎇ clean"));

    scenario.app.view = orkai::tui::app::View::Agent(id);
    let status = text(&render(&mut scenario.app)).lines().last().unwrap().to_string();
    assert!(status.contains(&format!("⎇ {}", worktree.branch)), "{status}");
}

#[test]
fn diff_shows_the_worktree_changes_and_closes_with_esc() {
    let mut scenario = scenario();
    scenario.app.run_command("/diff");
    assert_eq!(scenario.app.notice.as_ref().unwrap().text, "the selected agent does not run in its own worktree");

    let (id, worktree) = put_in_worktree(&mut scenario, "physics-merge");
    std::fs::write(worktree.path.join("readme.txt"), "hello\nworld\n").unwrap();
    std::fs::write(worktree.path.join("engine.rs"), "fn step() {}\n").unwrap();
    scenario.app.selected = Some(id);
    scenario.app.run_command("/diff");
    let event = next_event(&scenario, |e| matches!(e, AppEvent::Diff { .. }));
    scenario.app.handle(event);
    assert!(scenario.app.diff.is_some());
    let screen = text(&render(&mut scenario.app));
    assert!(screen.contains(&format!("diff physics-merge  ⎇ {} since", worktree.branch)), "{screen}");
    for expected in ["readme.txt", "+world", "engine.rs", "+fn step() {}", "esc close"] {
        assert!(screen.contains(expected), "missing {expected:?} in:\n{screen}");
    }
    assert!(!screen.contains("Running design update"), "the list is hidden behind the diff:\n{screen}");

    press(&mut scenario.app, KeyCode::PageDown, KeyModifiers::NONE);
    press(&mut scenario.app, KeyCode::Up, KeyModifiers::NONE);
    press(&mut scenario.app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(scenario.app.diff.is_none());
    assert!(text(&render(&mut scenario.app)).contains("Running design update"));

    scenario.app.handle(AppEvent::Diff { id, result: Ok(Vec::new()) });
    assert!(text(&render(&mut scenario.app)).contains("physics-merge has not changed anything yet"));
    press(&mut scenario.app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(scenario.app.diff.is_none());
    scenario.app.handle(AppEvent::Diff { id, result: Err("boom".into()) });
    assert_eq!(scenario.app.notice.as_ref().unwrap().text, "could not read the changes: boom");
}

#[test]
fn merge_asks_first_waits_for_the_agent_and_brings_the_work_home() {
    let mut scenario = scenario();
    let (id, worktree) = put_in_worktree(&mut scenario, "physics-merge");
    scenario.app.selected = Some(id);
    scenario.app.run_command("/merge");
    assert!(scenario.app.prompt.is_none());
    assert!(scenario.app.notice.as_ref().unwrap().text.contains("wait for physics-merge to finish"));

    push_state(&mut scenario.app, id, AgentState::Done);
    std::fs::write(worktree.path.join("engine.rs"), "fn step() {}\n").unwrap();
    let pending = Changes { files: 1, insertions: 1, deletions: 0 };
    scenario.app.handle(AppEvent::Changes(BTreeMap::from([(id, pending)])));
    scenario.app.run_command("/merge");
    assert!(matches!(scenario.app.prompt, Some(Prompt::Confirm(Confirm::Merge(target))) if target == id));
    let screen = text(&render(&mut scenario.app));
    assert!(screen.contains("merge worktree"), "{screen}");
    assert!(screen.contains(&format!("merge {} (1 files +1 −0)", worktree.branch)), "{screen}");
    press(&mut scenario.app, KeyCode::Char('n'), KeyModifiers::NONE);
    assert!(scenario.app.prompt.is_none());

    scenario.app.run_command("/merge");
    press(&mut scenario.app, KeyCode::Char('y'), KeyModifiers::NONE);
    let event = next_event(&scenario, |e| matches!(e, AppEvent::Merged { .. }));
    scenario.app.handle(event);
    assert_eq!(scenario.app.notice.as_ref().unwrap().text, format!("{} merged into main", worktree.branch));
    assert_eq!(std::fs::read_to_string(worktree.repo.join("engine.rs")).unwrap(), "fn step() {}\n");
    let saved = scenario.app.launcher.store().load_agent(id).unwrap().worktree.unwrap();
    assert_eq!(saved.base, git(&worktree.path, &["rev-parse", "HEAD"]));
    assert_eq!(scenario.app.agents[&id].changes, Some(Changes::default()));

    scenario.app.handle(AppEvent::Merged { id, result: Err("1 file(s) conflict with main".into()) });
    assert_eq!(scenario.app.notice.as_ref().unwrap().text, "merge failed: 1 file(s) conflict with main");
}

#[test]
fn deleting_an_agent_in_a_worktree_deletes_its_worktree_and_branch() {
    let mut scenario = scenario();
    let (id, worktree) = put_in_worktree(&mut scenario, "folder-icon-update");
    scenario.app.agents.get_mut(&id).unwrap().link =
        Link::Exited(ExitInfo { code: Some(0), error: None, at_ms: now_ms() });
    scenario.app.selected = Some(id);
    press(&mut scenario.app, KeyCode::Delete, KeyModifiers::NONE);
    let screen = text(&render(&mut scenario.app));
    assert!(screen.contains(&format!("worktree and branch {} go too", worktree.branch)), "{screen}");
    press(&mut scenario.app, KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(!scenario.app.agents.contains_key(&id));
    let event = next_event(&scenario, |e| matches!(e, AppEvent::Discarded { .. }));
    scenario.app.handle(event);
    assert_eq!(scenario.app.notice.as_ref().unwrap().text, "folder-icon-update removed with its worktree and branch");
    assert!(!worktree.path.exists());
    assert!(git(&worktree.repo, &["branch", "--list", &worktree.branch]).is_empty());
}

#[test]
fn a_failed_launch_in_a_new_worktree_leaves_nothing_behind() {
    let mut scenario = scenario();
    let repo = git_repo(&scenario.dir);
    scenario.app.where_dir = repo.clone();
    without_agent_programs(&mut scenario.app);
    let home = scenario.app.launcher.store().home().clone();
    scenario.app.run_command("/worktree on");

    type_text(&mut scenario.app, "!git status");
    press(&mut scenario.app, KeyCode::Enter, KeyModifiers::NONE);
    let notice = scenario.app.notice.as_ref().unwrap().text.clone();
    assert_eq!(notice, "starting terminal git-status…", "terminals never get a worktree");
    let _ = launched(&scenario);

    type_text(&mut scenario.app, "refactor the engine");
    press(&mut scenario.app, KeyCode::Enter, KeyModifiers::NONE);
    let notice = scenario.app.notice.as_ref().unwrap().text.clone();
    assert_eq!(notice, "starting claude refactor-engine in a new worktree…");
    let error = launched(&scenario).unwrap_err();
    assert!(error.contains("orkai-missing-claude"), "{error}");
    let left: Vec<_> = std::fs::read_dir(home.worktrees_dir()).map(|d| d.flatten().collect()).unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    assert!(git(&repo, &["branch", "--list", "orkai/*"]).is_empty());
    assert!(!git(&repo, &["worktree", "list"]).contains("worktrees"));
}
