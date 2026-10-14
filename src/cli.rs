use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use crate::client;
use crate::holder::oplog;
use crate::launch::{LaunchMode, Launcher};
use crate::model::{AgentId, AgentKind, AgentRecord, AgentState, ExitInfo, NewAgent, StatusReport};
use crate::paths::user_home;
use crate::protocol::{ClientMsg, HolderMsg, TermSize};
use crate::store::Store;
use crate::termfmt;
use crate::tui::format::{display_path, duration_short, pad, slugify, truncate};
use crate::tui::keys;
use crate::util::now_ms;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const KILL_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_SIZE: TermSize = TermSize { rows: 40, cols: 120 };
const SUBMIT_DELAY: Duration = Duration::from_millis(600);

pub enum Probe {
    Live(StatusReport),
    Exited(ExitInfo),
    Lost,
}

impl Probe {
    pub fn state(&self) -> AgentState {
        match self {
            Probe::Live(status) => status.state,
            Probe::Exited(_) => AgentState::Exited,
            Probe::Lost => AgentState::Lost,
        }
    }
}

fn not_found(id: AgentId) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("agent {id} not found"))
}

pub fn probe(store: &Store, id: AgentId) -> Probe {
    if let Ok(Some(exit)) = store.read_exit(id) {
        return Probe::Exited(exit);
    }
    let Ok(Some(runtime)) = store.read_runtime(id) else {
        return Probe::Lost;
    };
    let Ok((_conn, mut reader, _)) = client::connect(&runtime) else {
        return Probe::Lost;
    };
    let _ = reader.set_read_timeout(Some(PROBE_TIMEOUT));
    let deadline = Instant::now() + PROBE_TIMEOUT;
    while Instant::now() < deadline {
        match HolderMsg::read(&mut reader) {
            Ok(HolderMsg::Status(status)) => return Probe::Live(status),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Probe::Lost
}

pub fn list(store: &Store, session: Option<&str>) -> io::Result<()> {
    let home = user_home();
    let records: Vec<AgentRecord> =
        store.list_agents()?.into_iter().filter(|r| session.is_none_or(|s| r.session == s)).collect();
    if records.is_empty() {
        println!("no agents");
        return Ok(());
    }
    let now = now_ms();
    println!(
        "{:>4}  {}  {}  {}  {}  {:>7}  {}  ACTIVITY",
        "ID",
        pad("SESSION", 12),
        pad("STATE", 8),
        pad("KIND", 8),
        pad("NAME", 24),
        "UPTIME",
        pad("WHERE", 28)
    );
    for record in records {
        let probe = probe(store, record.id);
        let (uptime, activity) = match &probe {
            Probe::Live(status) => (duration_short(now.saturating_sub(status.started_at_ms)), status.activity.clone()),
            Probe::Exited(exit) => (String::new(), exit.describe()),
            Probe::Lost => (String::new(), "interrupted — run `orkai recover`".to_string()),
        };
        println!(
            "{:>4}  {}  {}  {}  {}  {:>7}  {}  {}",
            record.id,
            pad(&record.session, 12),
            pad(probe.state().label(), 8),
            pad(record.kind.label(), 8),
            pad(&record.name, 24),
            uptime,
            pad(&display_path(&record.cwd, home.as_deref()), 28),
            truncate(&activity, 60)
        );
    }
    Ok(())
}

pub fn sessions(store: &Store) -> io::Result<()> {
    let records = store.list_agents()?;
    for session in store.load_sessions_with(&records)? {
        let count = records.iter().filter(|r| r.session == session.name).count();
        println!("{}  {count}", pad(&session.name, 24));
    }
    Ok(())
}

pub fn new_agent(
    launcher: &Launcher,
    kind: AgentKind,
    session: String,
    cwd: Option<PathBuf>,
    name: Option<String>,
    task: String,
) -> io::Result<()> {
    let cwd = match cwd {
        Some(dir) => dir.canonicalize()?,
        None => std::env::current_dir()?,
    };
    let name = name.unwrap_or_else(|| {
        let slug = slugify(&task, 3, 28);
        if slug.is_empty() {
            format!("{}-{}", if kind == AgentKind::Terminal { "term" } else { kind.label() }, now_ms() % 10_000)
        } else {
            slug
        }
    });
    let record = launcher.create(NewAgent { session, name, kind, cwd: strip_verbatim(cwd), task }, DEFAULT_SIZE)?;
    println!("started {} {} (#{}) in session {}", record.kind.label(), record.name, record.id, record.session);
    Ok(())
}

fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

pub fn kill(store: &Store, id: AgentId) -> io::Result<()> {
    store.load_agent(id).map_err(|_| not_found(id))?;
    let runtime = store.read_runtime(id)?.ok_or_else(|| io::Error::other(format!("agent {id} is not running")))?;
    let (conn, _reader, _) = client::connect(&runtime)?;
    conn.send(&ClientMsg::Kill)?;
    let deadline = Instant::now() + KILL_TIMEOUT;
    while Instant::now() < deadline {
        if store.read_exit(id)?.is_some() {
            println!("agent {id} stopped");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "agent did not stop in time"))
}

pub fn send(store: &Store, id: AgentId, text: &str, submit: bool) -> io::Result<()> {
    store.load_agent(id).map_err(|_| not_found(id))?;
    let runtime = store.read_runtime(id)?.ok_or_else(|| io::Error::other(format!("agent {id} is not running")))?;
    let (conn, _reader, _) = client::connect(&runtime)?;
    if !text.is_empty() {
        conn.send(&ClientMsg::Input(text.as_bytes().to_vec()))?;
    }
    if submit {
        std::thread::sleep(SUBMIT_DELAY);
        conn.send(&ClientMsg::Input(b"\r".to_vec()))?;
    }
    std::thread::sleep(Duration::from_millis(100));
    Ok(())
}

pub fn remove(store: &Store, id: AgentId) -> io::Result<()> {
    store.load_agent(id).map_err(|_| not_found(id))?;
    if let Probe::Live(_) = probe(store, id) {
        return Err(io::Error::other(format!("agent {id} is running; stop it first with `orkai kill {id}`")));
    }
    store.remove_agent(id)?;
    println!("agent {id} removed");
    Ok(())
}

pub fn recover(launcher: &Launcher, session: Option<&str>) -> io::Result<()> {
    let store = launcher.store();
    let mut recovered = 0;
    for mut record in store.list_agents()? {
        if session.is_some_and(|s| record.session != s) {
            continue;
        }
        if !matches!(probe(store, record.id), Probe::Lost) {
            continue;
        }
        match launcher.start(&mut record, LaunchMode::Resume, DEFAULT_SIZE) {
            Ok(()) => {
                recovered += 1;
                println!(
                    "recovered #{} {} ({}) in {}",
                    record.id,
                    record.name,
                    record.kind.label(),
                    record.cwd.display()
                );
            }
            Err(err) => eprintln!("could not recover #{} {}: {err}", record.id, record.name),
        }
    }
    if recovered == 0 {
        println!("nothing to recover");
    }
    Ok(())
}

pub fn logs(store: &Store, id: AgentId, lines: Option<usize>) -> io::Result<()> {
    store.load_agent(id).map_err(|_| not_found(id))?;
    let Some(mut parser) = oplog::replay(&store.home().output_log(id))? else {
        println!("(no output recorded)");
        return Ok(());
    };
    let transcript = termfmt::transcript(parser.screen_mut());
    let start = lines.map_or(0, |n| transcript.len().saturating_sub(n));
    let mut out = io::stdout().lock();
    for line in &transcript[start..] {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

struct RawGuard;

impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, ratatui::crossterm::cursor::Show);
    }
}

pub fn attach(store: &Store, id: AgentId) -> io::Result<()> {
    let record = store.load_agent(id).map_err(|_| not_found(id))?;
    let runtime = store
        .read_runtime(id)?
        .ok_or_else(|| io::Error::other(format!("agent {id} is not running; try `orkai recover`")))?;
    let (conn, reader, _) = client::connect(&runtime)?;
    let (cols, rows) = terminal::size()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    terminal::enable_raw_mode()?;
    let guard = RawGuard;
    conn.send(&ClientMsg::Attach(TermSize::new(rows, cols)))?;

    let app_cursor = Arc::new(AtomicBool::new(false));
    let bracketed = Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let mut parser = vt100::Parser::new(rows.max(2), cols.max(10), 0);
    let (cursor_flag, paste_flag) = (Arc::clone(&app_cursor), Arc::clone(&bracketed));
    client::spawn_reader(reader, move |msg| {
        let mut stdout = io::stdout().lock();
        let bytes = match msg {
            Some(HolderMsg::Snapshot(snapshot)) => {
                parser = vt100::Parser::new(snapshot.size.rows, snapshot.size.cols, 0);
                let mut data = b"\x1b[H\x1b[2J".to_vec();
                data.extend_from_slice(&snapshot.data);
                data
            }
            Some(HolderMsg::Output(bytes)) => bytes,
            Some(HolderMsg::Exited(_)) | None => {
                let _ = done_tx.send(());
                return false;
            }
            Some(_) => return true,
        };
        parser.process(&bytes);
        cursor_flag.store(parser.screen().application_cursor(), Ordering::Relaxed);
        paste_flag.store(parser.screen().bracketed_paste(), Ordering::Relaxed);
        stdout.write_all(&bytes).and_then(|()| stdout.flush()).is_ok()
    })?;

    loop {
        if done_rx.try_recv().is_ok() {
            break;
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                if keys::is_detach_key(&key) {
                    break;
                }
                if let Some(bytes) = keys::encode_key(&key, app_cursor.load(Ordering::Relaxed)) {
                    conn.send(&ClientMsg::Input(bytes))?;
                }
            }
            Event::Paste(text) => {
                conn.send(&ClientMsg::Input(keys::encode_paste(&text, bracketed.load(Ordering::Relaxed))))?;
            }
            Event::Resize(cols, rows) => conn.send(&ClientMsg::Resize(TermSize::new(rows, cols)))?,
            _ => {}
        }
    }
    let _ = conn.send(&ClientMsg::Detach);
    drop(guard);
    println!("detached from #{} {}", record.id, record.name);
    Ok(())
}
