pub mod app;
pub mod complete;
pub mod format;
pub mod history;
pub mod keys;
pub mod line;
pub mod render;
pub mod sprite;
pub mod theme;
pub mod view;

use std::io::{self, Stdout, Write};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::Rect;

use crate::launch::Launcher;
use crate::notify::{self, Notifier};
use crate::sound::{Chime, Player};
use crate::terminal_profile::{self, Switch};
use app::{App, AppEvent};

const TICK: Duration = Duration::from_millis(120);
const MIN_FRAME: Duration = Duration::from_millis(16);

type Term = Terminal<CrosstermBackend<Stdout>>;

pub const WINDOW_TITLE: &str = "Orkai";
const PUSH_TITLE: &str = "\x1b[22;0t";
const POP_TITLE: &str = "\x1b[23;0t";

static SAVED_TITLE: OnceLock<Option<String>> = OnceLock::new();
static SWITCHED_PROFILE: OnceLock<String> = OnceLock::new();

#[cfg(windows)]
fn current_title() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleTitleW(title: *mut u16, size: u32) -> u32;
    }
    let mut buffer = vec![0u16; 1024];
    let size = u32::try_from(buffer.len()).unwrap_or(0);
    let len = usize::try_from(unsafe { GetConsoleTitleW(buffer.as_mut_ptr(), size) }).unwrap_or(0);
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len.min(buffer.len())]))
}

#[cfg(not(windows))]
fn current_title() -> Option<String> {
    None
}

fn set_window_title() -> io::Result<()> {
    let mut stdout = io::stdout();
    if SAVED_TITLE.get_or_init(current_title).is_none() {
        stdout.write_all(PUSH_TITLE.as_bytes())?;
    }
    execute!(stdout, SetTitle(WINDOW_TITLE))
}

fn restore_window_title() {
    let Some(saved) = SAVED_TITLE.get() else { return };
    let mut stdout = io::stdout();
    match saved {
        Some(title) => {
            let _ = execute!(stdout, SetTitle(title));
        }
        None => {
            let _ = stdout.write_all(POP_TITLE.as_bytes()).and_then(|()| stdout.flush());
        }
    }
}

fn switch_profile(pending: &mut Option<Switch>, started: Instant) {
    let Some(switch) = pending.take_if(|switch| started.elapsed() >= switch.delay) else { return };
    let mut stdout = io::stdout();
    let sequence = terminal_profile::set_profile_sequence(&switch.to);
    if stdout.write_all(sequence.as_bytes()).and_then(|()| stdout.flush()).is_ok() {
        let _ = SWITCHED_PROFILE.set(switch.back);
    }
}

fn restore_profile() {
    let Some(back) = SWITCHED_PROFILE.get() else { return };
    let mut stdout = io::stdout();
    let _ = stdout.write_all(terminal_profile::set_profile_sequence(back).as_bytes()).and_then(|()| stdout.flush());
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste, LeaveAlternateScreen);
    let _ = execute!(io::stdout(), ratatui::crossterm::cursor::Show);
    restore_window_title();
    restore_profile();
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn setup_terminal() -> io::Result<(Term, TerminalGuard)> {
    enable_raw_mode()?;
    let guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    set_window_title()?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    Ok((terminal, guard))
}

fn spawn_input(tx: mpsc::Sender<AppEvent>) -> io::Result<()> {
    thread::Builder::new()
        .name("orkai-input".into())
        .spawn(move || {
            while let Ok(event) = event::read() {
                let queued = |wait| match event::poll(wait) {
                    Ok(true) => event::read().ok(),
                    _ => None,
                };
                for event in keys::coalesce_paste(event, queued) {
                    if tx.send(AppEvent::Input(event)).is_err() {
                        return;
                    }
                }
            }
        })
        .map(|_| ())
}

fn spawn_ticker(tx: mpsc::Sender<AppEvent>) -> io::Result<()> {
    thread::Builder::new()
        .name("orkai-ticker".into())
        .spawn(move || {
            loop {
                thread::sleep(TICK);
                if tx.send(AppEvent::Tick).is_err() {
                    return;
                }
            }
        })
        .map(|_| ())
}

fn deliver_alerts(app: &mut App, notifier: &Notifier, ring: bool, player: Option<&Player>) {
    let alerts = app.take_alerts();
    if alerts.is_empty() {
        return;
    }
    if ring {
        let chimed = player.zip(Chime::for_alerts(&alerts)).is_some_and(|(player, chime)| player.play(chime));
        let mut stdout = io::stdout();
        let _ = stdout.write_all(&notify::terminal_sequence(&alerts, !chimed)).and_then(|()| stdout.flush());
    }
    for alert in alerts {
        notifier.send(alert);
    }
}

fn sync_window_title(app: &App, shown: &mut String) {
    let title = app.window_title();
    if *shown != title && execute!(io::stdout(), SetTitle(&title)).is_ok() {
        *shown = title;
    }
}

fn drain(app: &mut App, rx: &Receiver<AppEvent>) {
    while let Ok(event) = rx.try_recv() {
        app.handle(event);
        if app.should_quit {
            return;
        }
    }
}

pub fn run(launcher: Launcher) -> io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let notifier = Notifier::start(launcher.config());
    let ring = launcher.config().notify;
    let player = if launcher.config().notify_sound { Player::new(launcher.store().home()) } else { None };
    let mut profile =
        std::env::current_exe().ok().and_then(|exe| terminal_profile::install(launcher.store().home(), &exe));
    let started = Instant::now();
    let (mut terminal, guard) = setup_terminal()?;
    let size = terminal.size()?;
    let mut app = App::new(launcher, tx.clone(), Rect::new(0, 0, size.width, size.height))?;
    spawn_input(tx.clone())?;
    spawn_ticker(tx)?;
    app.start();
    terminal.draw(|frame| render::draw(frame, &mut app))?;
    let mut last_draw = Instant::now();
    let mut title = WINDOW_TITLE.to_string();
    while !app.should_quit {
        match rx.recv() {
            Ok(event) => app.handle(event),
            Err(_) => break,
        }
        drain(&mut app, &rx);
        app.flush_input();
        app.sync_attached_size();
        deliver_alerts(&mut app, &notifier, ring, player.as_ref());
        switch_profile(&mut profile, started);
        if !app.take_dirty() {
            continue;
        }
        let since = last_draw.elapsed();
        if since < MIN_FRAME {
            match rx.recv_timeout(MIN_FRAME - since) {
                Ok(event) => {
                    app.handle(event);
                    drain(&mut app, &rx);
                    app.flush_input();
                    app.sync_attached_size();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        app.take_dirty();
        terminal.draw(|frame| render::draw(frame, &mut app))?;
        sync_window_title(&app, &mut title);
        last_draw = Instant::now();
    }
    app.shutdown();
    drop(terminal);
    drop(guard);
    Ok(())
}
