use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

use orkai::config::Config;
use orkai::launch::Launcher;
use orkai::model::{AgentKind, AgentState, NewAgent, StatusReport};
use orkai::paths::Home;
use orkai::store::Store;
use orkai::tui::app::{App, Link};
use orkai::tui::render;
use orkai::util::now_ms;

const WIDTH: u16 = 150;
const HEIGHT: u16 = 40;
static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Scenario {
    app: App,
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
    let (tx, _rx) = mpsc::channel();
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
    Scenario { app, dir }
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
