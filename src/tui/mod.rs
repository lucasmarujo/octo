pub mod app;
pub mod format;
pub mod keys;
pub mod line;
pub mod render;
pub mod sprite;
pub mod theme;
pub mod view;

use std::io::{self, Stdout};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::layout::Rect;

use crate::launch::Launcher;
use app::{App, AppEvent};

const TICK: Duration = Duration::from_millis(120);
const MIN_FRAME: Duration = Duration::from_millis(16);

type Term = Terminal<CrosstermBackend<Stdout>>;

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste, LeaveAlternateScreen);
    let _ = execute!(io::stdout(), ratatui::crossterm::cursor::Show);
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
                if tx.send(AppEvent::Input(event)).is_err() {
                    return;
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
    let (mut terminal, guard) = setup_terminal()?;
    let size = terminal.size()?;
    let mut app = App::new(launcher, tx.clone(), Rect::new(0, 0, size.width, size.height))?;
    spawn_input(tx.clone())?;
    spawn_ticker(tx)?;
    app.start();
    terminal.draw(|frame| render::draw(frame, &mut app))?;
    let mut last_draw = Instant::now();
    while !app.should_quit {
        match rx.recv() {
            Ok(event) => app.handle(event),
            Err(_) => break,
        }
        drain(&mut app, &rx);
        app.flush_input();
        app.sync_attached_size();
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
        last_draw = Instant::now();
    }
    app.shutdown();
    drop(terminal);
    drop(guard);
    Ok(())
}
