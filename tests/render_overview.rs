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
use orkai::model::{AgentKind, AgentRecord, AgentState, ExitInfo, NewAgent, StatusReport};
use orkai::paths::Home;
use orkai::store::Store;
use orkai::subagents::Subagent;
use orkai::tui::app::{App, AppEvent, Confirm, Link, Prompt};
use orkai::tui::{render, theme};
use orkai::usage::{Limits, Problem, Window};
use orkai::util::now_ms;

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
