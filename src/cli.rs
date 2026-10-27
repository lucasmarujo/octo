use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use crate::client;
use crate::holder::oplog;
use serde::Serialize;

use crate::launch::{AGENT_ID_ENV, LaunchMode, Launcher};
use crate::model::{AgentId, AgentKind, AgentRecord, AgentState, ExitInfo, NewAgent, StatusReport, TestState};
use crate::paths::{Home, user_home};
use crate::project;
use crate::protocol::{ClientMsg, HolderMsg, TermSize};
use crate::pwa;
use crate::pwa::bridge::Phase;
use crate::pwa::tunnel::TunnelKind;
use crate::store::Store;
use crate::termfmt;
use crate::tui::format::{display_path, duration_short, pad, slugify, truncate};
use crate::tui::keys;
use crate::util::now_ms;
use crate::worktree;

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const KILL_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_SIZE: TermSize = TermSize { rows: 40, cols: 120 };
pub const SUBMIT_DELAY: Duration = Duration::from_millis(600);
const PWA_READY_WAIT: Duration = Duration::from_secs(360);
const PWA_STOP_WAIT: Duration = Duration::from_secs(10);
const RECONNECT_DELAY: Duration = Duration::from_millis(200);

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

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentListing {
    pub id: AgentId,
    pub session: String,
    pub name: String,
    pub kind: AgentKind,
    pub state: AgentState,
    pub cwd: PathBuf,
    pub project: PathBuf,
    pub branch: Option<String>,
    pub parent: Option<AgentId>,
    pub task: String,
    pub activity: String,
    pub test: Option<TestState>,
    pub created_at_ms: u64,
}

pub fn listing(store: &Store, session: Option<&str>) -> io::Result<Vec<AgentListing>> {
    let records = store.list_agents()?.into_iter().filter(|r| session.is_none_or(|s| r.session == s));
    Ok(records
        .map(|record| {
            let probe = probe(store, record.id);
            let (kind, activity, test) = match &probe {
                Probe::Live(status) => (
                    status.running_kind(record.kind),
                    status.activity.clone(),
                    status.test.as_ref().map(|test| test.state),
                ),
                Probe::Exited(exit) => (record.kind, exit.describe(), None),
                Probe::Lost => (record.kind, String::new(), None),
            };
            AgentListing {
                id: record.id,
                state: probe.state(),
                kind,
                project: record.project_dir().to_path_buf(),
                branch: record.worktree.as_ref().map(|worktree| worktree.branch.clone()),
                parent: record.parent,
                activity,
                test,
                created_at_ms: record.created_at_ms,
                session: record.session,
                name: record.name,
                cwd: record.cwd,
                task: record.task,
            }
        })
        .collect())
}

pub fn list_json(store: &Store, session: Option<&str>) -> io::Result<()> {
    let agents = listing(store, session)?;
    println!("{}", serde_json::to_string_pretty(&agents).map_err(io::Error::other)?);
    Ok(())
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
        let (uptime, activity, kind) = match &probe {
            Probe::Live(status) => (
                duration_short(now.saturating_sub(status.started_at_ms)),
                status.activity.clone(),
                status.running_kind(record.kind),
            ),
            Probe::Exited(exit) => (String::new(), exit.describe(), record.kind),
            Probe::Lost => (String::new(), "interrupted — run `orkai recover`".to_string(), record.kind),
        };
        println!(
            "{:>4}  {}  {}  {}  {}  {:>7}  {}  {}",
            record.id,
            pad(&record.session, 12),
            pad(probe.state().label(), 8),
            pad(kind.label(), 8),
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
    isolate: bool,
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
    let parent = std::env::var(AGENT_ID_ENV)
        .ok()
        .and_then(|id| id.parse::<AgentId>().ok())
        .filter(|id| launcher.store().load_agent(*id).is_ok());
    let record = launcher.create_with_parent(
        NewAgent { session, name, kind, cwd: strip_verbatim(cwd), task },
        DEFAULT_SIZE,
        isolate,
        false,
        parent,
    )?;
    println!("started {} {} (#{}) in session {}", record.kind.label(), record.name, record.id, record.session);
    if let Some(worktree) = &record.worktree {
        println!("worktree {} on branch {}", record.cwd.display(), worktree.branch);
    }
    Ok(())
}

pub fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

pub fn stop(store: &Store, id: AgentId) -> io::Result<()> {
    store.load_agent(id).map_err(|_| not_found(id))?;
    let runtime = store.read_runtime(id)?.ok_or_else(|| io::Error::other(format!("agent {id} is not running")))?;
    let (conn, _reader, _) = client::connect(&runtime)?;
    conn.send(&ClientMsg::Kill)?;
    let deadline = Instant::now() + KILL_TIMEOUT;
    while Instant::now() < deadline {
        if store.read_exit(id)?.is_some() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "agent did not stop in time"))
}

pub fn kill(store: &Store, id: AgentId) -> io::Result<()> {
    stop(store, id)?;
    println!("agent {id} stopped");
    Ok(())
}

pub enum Waited {
    Settled(StatusReport),
    Exited(ExitInfo),
    Lost,
    TimedOut(Option<StatusReport>),
}

pub fn settled(status: &StatusReport, since: u64, saw_working: bool) -> bool {
    match status.state {
        AgentState::Working => false,
        AgentState::Paused | AgentState::Done => status.state_since_ms >= since,
        AgentState::Idle => (status.last_turn_ms.is_some() || saw_working) && status.state_since_ms >= since,
        AgentState::Exited | AgentState::Lost => true,
    }
}

pub fn wait_turn(store: &Store, id: AgentId, since: u64, timeout: Option<Duration>) -> io::Result<Waited> {
    store.load_agent(id).map_err(|_| not_found(id))?;
    let deadline = timeout.map(|limit| Instant::now() + limit);
    let mut last = None;
    let mut saw_working = false;
    loop {
        if let Some(exit) = store.read_exit(id)? {
            return Ok(Waited::Exited(exit));
        }
        let Some(runtime) = store.read_runtime(id)? else { return Ok(Waited::Lost) };
        if let Ok((conn, reader, _)) = client::connect(&runtime) {
            let (tx, rx) = mpsc::channel();
            client::spawn_reader(reader, move |msg| tx.send(msg).is_ok())?;
            loop {
                let received = match deadline.map(|d| d.saturating_duration_since(Instant::now())) {
                    Some(remaining) if remaining.is_zero() => {
                        conn.close();
                        return Ok(Waited::TimedOut(last));
                    }
                    Some(remaining) => match rx.recv_timeout(remaining) {
                        Ok(msg) => msg,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => None,
                    },
                    None => rx.recv().ok().flatten(),
                };
                match received {
                    Some(HolderMsg::Status(status)) => {
                        saw_working |= status.state == AgentState::Working;
                        if settled(&status, since, saw_working) {
                            return Ok(Waited::Settled(status));
                        }
                        last = Some(status);
                    }
                    Some(HolderMsg::Exited(exit)) => return Ok(Waited::Exited(exit)),
                    Some(_) => {}
                    None => break,
                }
            }
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Ok(Waited::TimedOut(last));
        }
        thread::sleep(RECONNECT_DELAY);
    }
}

pub fn wait(store: &Store, id: AgentId, timeout: Option<u64>) -> io::Result<()> {
    match wait_turn(store, id, 0, timeout.map(Duration::from_secs))? {
        Waited::Settled(status) => {
            println!("agent {id} {}: {}", status.state.label(), status.activity);
            Ok(())
        }
        Waited::Exited(exit) => {
            println!("agent {id} {}", exit.describe());
            Ok(())
        }
        Waited::Lost => Err(io::Error::other(format!("agent {id} is interrupted, run orkai recover"))),
        Waited::TimedOut(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("agent {id} is still working after {}s", timeout.unwrap_or_default()),
        )),
    }
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
    let worktree = store.load_agent(id)?.worktree;
    store.remove_agent(id)?;
    println!("agent {id} removed");
    if let Some(worktree) = worktree {
        worktree::discard(store.home(), &worktree)?;
        println!("worktree {} and branch {} deleted", worktree.path.display(), worktree.branch);
    }
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

pub fn trust(store: &Store, cwd: Option<PathBuf>) -> io::Result<()> {
    let dir = match cwd {
        Some(dir) => dir,
        None => std::env::current_dir()?,
    };
    let repo = worktree::toplevel(&dir)?;
    let loaded = project::load(&repo)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("{} has no {}", repo.display(), project::FILE))
    })?;
    let command = loaded.config.test_command().map(str::to_string).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("{} has no test command", loaded.path.display()))
    })?;
    project::trust(store.home(), &loaded)?;
    println!("trusted {}: agents in worktrees of this repository run {command} after each turn", loaded.path.display());
    Ok(())
}

pub fn transcript(store: &Store, id: AgentId) -> io::Result<Option<Vec<String>>> {
    store.load_agent(id).map_err(|_| not_found(id))?;
    Ok(oplog::replay(&store.home().output_log(id))?.map(|mut parser| termfmt::transcript(parser.screen_mut())))
}

pub fn logs(store: &Store, id: AgentId, lines: Option<usize>) -> io::Result<()> {
    let Some(transcript) = transcript(store, id)? else {
        println!("(no output recorded)");
        return Ok(());
    };
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

pub enum PwaAction {
    Show,
    Stop,
    NewCode,
}

fn set_pwa_enabled(store: &Store, enabled: bool) -> io::Result<()> {
    let mut ui = store.load_ui();
    if ui.pwa != enabled {
        ui.pwa = enabled;
        store.save_ui(&ui)?;
    }
    Ok(())
}

fn stop_pwa(home: &Home) -> io::Result<()> {
    pwa::bridge::stop(home)?;
    if !pwa::bridge::wait_until(home, PWA_STOP_WAIT, |info| info.is_none()) {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "the orkai pwa did not stop in time"));
    }
    Ok(())
}

pub fn pwa(launcher: &Launcher, tunnel: Option<TunnelKind>, action: PwaAction) -> io::Result<()> {
    let store = launcher.store();
    let home = store.home();
    match action {
        PwaAction::Stop => {
            set_pwa_enabled(store, false)?;
            stop_pwa(home)?;
            println!("orkai pwa is off");
            return Ok(());
        }
        PwaAction::NewCode => {
            stop_pwa(home)?;
            pwa::bridge::forget_phones(home)?;
            println!("the paired phones were forgotten; scan the new code");
        }
        PwaAction::Show => {}
    }
    let config = launcher.config();
    let tunnel = tunnel.unwrap_or(config.pwa_tunnel);
    set_pwa_enabled(store, true)?;
    pwa::bridge::start(home, launcher.holder_exe(), tunnel, config.pwa_port)?;
    let mut shown = None;
    let ready = pwa::bridge::wait_until(home, PWA_READY_WAIT, |info| {
        let Some(info) = info else { return false };
        if shown != Some(info.phase) {
            shown = Some(info.phase);
            match info.phase {
                Phase::Installing => {
                    println!("downloading cloudflared (the official build from github.com/cloudflare)…")
                }
                Phase::Connecting => println!("opening the {} tunnel…", info.tunnel.label()),
                Phase::Ready | Phase::Failed => {}
            }
        }
        matches!(info.phase, Phase::Ready | Phase::Failed)
    });
    let info = pwa::bridge::status(home).filter(|_| ready);
    let Some(url) = info.as_ref().and_then(|info| info.ready_url()) else {
        let error = info.and_then(|info| info.error).unwrap_or_else(|| "the tunnel did not open in time".into());
        return Err(io::Error::other(format!("orkai pwa: {error}")));
    };
    let link = pwa::bridge::pairing_link(url, &pwa::bridge::load_secret(home)?);
    println!();
    for line in pwa::qr::Qr::encode(&link).map(|qr| qr.ansi_lines()).unwrap_or_default() {
        println!("  {line}");
    }
    println!();
    println!("scan it with the phone camera, or open: {link}");
    println!("end-to-end encrypted over {} · `orkai pwa --stop` turns it off", tunnel.label());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: AgentState, since: u64, last_turn: Option<u64>) -> StatusReport {
        StatusReport {
            state,
            state_since_ms: since,
            turn_started_ms: None,
            last_turn_ms: last_turn,
            started_at_ms: 1,
            title: String::new(),
            activity: String::new(),
            rows: 24,
            cols: 80,
            exit: None,
            nested: None,
            viewers: None,
            test: None,
            input_at_ms: 0,
        }
    }

    #[test]
    fn a_turn_counts_as_settled_once_it_ends_after_the_wait_began() {
        assert!(!settled(&status(AgentState::Working, 500, None), 0, true));
        assert!(settled(&status(AgentState::Done, 500, Some(10)), 0, false));
        assert!(settled(&status(AgentState::Paused, 500, None), 0, false), "waiting for an answer ends the wait");
        assert!(settled(&status(AgentState::Idle, 500, Some(10)), 0, false), "a finished turn already seen");
        assert!(!settled(&status(AgentState::Idle, 500, None), 0, false), "a fresh agent has not worked yet");
        assert!(settled(&status(AgentState::Idle, 500, None), 0, true), "it worked while we waited");

        assert!(!settled(&status(AgentState::Done, 500, Some(10)), 900, false), "an older turn than the message");
        assert!(!settled(&status(AgentState::Paused, 500, None), 900, false));
        assert!(settled(&status(AgentState::Done, 950, Some(10)), 900, true));
    }
}
