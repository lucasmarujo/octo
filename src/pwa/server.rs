use std::io::{self, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Message, Role, WebSocket, WebSocketConfig};

use crate::cli::SUBMIT_DELAY;
use crate::client::{self, Connection};
use crate::holder::oplog;
use crate::holder::term::encode_typed_text;
use crate::model::AgentId;
use crate::protocol::{ClientMsg, HolderMsg, TermSize};
use crate::pwa::actions;
use crate::pwa::crypto::{self, Channel, Ephemeral, Secret, Side};
use crate::pwa::http;
use crate::pwa::hub::{Event, Hub, encode};
use crate::pwa::protocol::{BridgeMsg, Greeting, PhoneMsg, Tone, VERSION};
use crate::pwa::screen::Mirror;
use crate::tui::keys;
use crate::tui::view::TermView;

pub const MAX_CONNECTIONS: usize = 8;
pub const UNPAIRED: u16 = 4001;
pub const OUTDATED: u16 = 4002;
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
const TICK: Duration = Duration::from_millis(25);
const PING_EVERY: Duration = Duration::from_secs(20);
const SILENCE_LIMIT: Duration = Duration::from_secs(70);
const SCREEN_EVERY: Duration = Duration::from_millis(40);
const MAX_INBOUND_BYTES: usize = 256 * 1024;
const MIN_SIZE: TermSize = TermSize { rows: 8, cols: 20 };
const MAX_SIZE: TermSize = TermSize { rows: 200, cols: 400 };
const SUBMIT: &[u8] = b"\r";
const WHEEL_LINES: i32 = 3;
const MAX_WHEEL_LINES: i32 = 200;

pub fn loopback(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

pub struct Server {
    listener: TcpListener,
    hub: Arc<Hub>,
    secret: Secret,
}

impl Server {
    pub fn bind(port: u16, hub: Arc<Hub>, secret: Secret) -> io::Result<Self> {
        Ok(Self { listener: TcpListener::bind(loopback(port))?, hub, secret })
    }

    pub fn port(&self) -> u16 {
        self.listener.local_addr().map(|address| address.port()).unwrap_or(0)
    }

    pub fn serve(self) {
        let active = Arc::new(AtomicUsize::new(0));
        for stream in self.listener.incoming() {
            let Ok(stream) = stream else { continue };
            if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                active.fetch_sub(1, Ordering::SeqCst);
                let _ = stream.shutdown(Shutdown::Both);
                continue;
            }
            let (hub, secret, counter) = (Arc::clone(&self.hub), self.secret, Arc::clone(&active));
            let spawned = thread::Builder::new().name("orkai-pwa-conn".into()).spawn(move || {
                let _ = handle(&hub, &secret, stream);
                counter.fetch_sub(1, Ordering::SeqCst);
            });
            if spawned.is_err() {
                active.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }
}

fn handle(hub: &Arc<Hub>, secret: &Secret, stream: TcpStream) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HEAD_TIMEOUT))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let request = http::read_request(&mut &stream)?;
    if request.wants_socket() {
        return upgrade(hub, secret, stream, &request);
    }
    http::write_response(&mut &stream, &http::respond(&request), request.method == "HEAD")
}

fn upgrade(hub: &Arc<Hub>, secret: &Secret, mut stream: TcpStream, request: &http::Request) -> io::Result<()> {
    let key = request.header("sec-websocket-key").filter(|key| !key.is_empty());
    let (Some(key), Some("13")) = (key, request.header("sec-websocket-version")) else {
        return http::write_response(&mut stream, &http::Response::text(400, "bad websocket request"), false);
    };
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    stream.flush()?;
    stream.set_read_timeout(Some(TICK))?;
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_INBOUND_BYTES);
    config.max_frame_size = Some(MAX_INBOUND_BYTES);
    let ws = WebSocket::from_raw_socket(stream, Role::Server, Some(config));
    let Some(mut phone) = Phone::pair(ws, Arc::clone(hub), secret)? else { return Ok(()) };
    let result = phone.run();
    hub.leave(phone.id);
    result
}

fn is_tick(err: &tungstenite::Error) -> bool {
    matches!(err, tungstenite::Error::Io(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut))
}

fn ws_error(err: tungstenite::Error) -> io::Error {
    match err {
        tungstenite::Error::Io(err) => err,
        other => io::Error::other(other.to_string()),
    }
}

fn next_message(ws: &mut WebSocket<TcpStream>, deadline: Instant) -> io::Result<Message> {
    loop {
        match ws.read() {
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Ok(message) => return Ok(message),
            Err(err) if is_tick(&err) && Instant::now() < deadline => {}
            Err(err) if is_tick(&err) => {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "the phone did not finish the pairing"));
            }
            Err(err) => return Err(ws_error(err)),
        }
    }
}

fn close(mut ws: WebSocket<TcpStream>, code: u16, reason: &str) {
    let frame = CloseFrame { code: CloseCode::from(code), reason: reason.to_string().into() };
    let _ = ws.close(Some(frame));
    let _ = ws.flush();
}

fn clamp(rows: u16, cols: u16) -> TermSize {
    TermSize { rows: rows.clamp(MIN_SIZE.rows, MAX_SIZE.rows), cols: cols.clamp(MIN_SIZE.cols, MAX_SIZE.cols) }
}

struct View {
    agent: AgentId,
    generation: u64,
    conn: Option<Arc<Connection>>,
    term: Option<TermView>,
    mirror: Mirror,
    want: TermSize,
    pty: Option<TermSize>,
    viewers: Option<u16>,
    requested: Option<TermSize>,
    dirty: bool,
    flushed: Instant,
    submit_at: Option<Instant>,
    wheel: i32,
}

impl View {
    fn target(&self) -> Option<TermSize> {
        let pty = self.pty?;
        Some(if self.viewers.is_some() { self.want } else { pty })
    }

    fn scroll(&mut self, lines: i32) {
        if lines != 0
            && let Some(conn) = self.conn.clone()
            && let Some(ticks) = self.wheel(lines)
        {
            if !ticks.is_empty() {
                let _ = conn.send(&ClientMsg::Input(ticks));
            }
            return;
        }
        let Some(term) = self.term.as_mut() else { return };
        match lines {
            0 => term.scroll_to_bottom(),
            lines => term.scroll_by(isize::try_from(lines).unwrap_or(0)),
        }
        self.dirty = true;
    }

    fn wheel(&mut self, lines: i32) -> Option<Vec<u8>> {
        let screen = self.term.as_ref().filter(|term| !term.is_scrolled())?.screen();
        let (mode, encoding) = (screen.mouse_protocol_mode(), screen.mouse_protocol_encoding());
        if mode == vt100::MouseProtocolMode::None {
            return None;
        }
        let pending = self.wheel.saturating_add(lines).clamp(-MAX_WHEEL_LINES, MAX_WHEEL_LINES);
        self.wheel = pending % WHEEL_LINES;
        let ticks = pending / WHEEL_LINES;
        let kind = if ticks > 0 { MouseEventKind::ScrollUp } else { MouseEventKind::ScrollDown };
        let event = MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::NONE };
        let (rows, cols) = screen.size();
        let tick = keys::encode_mouse(&event, cols / 2, rows / 2, mode, encoding)?;
        Some(tick.repeat(usize::try_from(ticks.unsigned_abs()).unwrap_or(0)))
    }
}

struct Phone {
    id: u64,
    ws: WebSocket<TcpStream>,
    channel: Channel,
    hub: Arc<Hub>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    view: Option<View>,
    generation: u64,
    heard: Instant,
    pinged: Instant,
}

impl Phone {
    fn pair(mut ws: WebSocket<TcpStream>, hub: Arc<Hub>, secret: &Secret) -> io::Result<Option<Self>> {
        let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        let Message::Text(text) = next_message(&mut ws, deadline)? else {
            close(ws, OUTDATED, "expected the hello");
            return Ok(None);
        };
        let peer = match serde_json::from_str::<Greeting>(text.as_str()) {
            Ok(Greeting::Hello { version: VERSION, key }) => crypto::decode_public_key(&key),
            _ => None,
        };
        let Some(peer) = peer else {
            close(ws, OUTDATED, "this orkai speaks another pwa protocol; reload the app");
            return Ok(None);
        };
        let own = Ephemeral::generate().map_err(io::Error::other)?;
        let key = crypto::encode(own.public_key());
        let mut channel = Channel::establish(Side::Bridge, own, &peer, secret).map_err(io::Error::other)?;
        let hello = serde_json::to_string(&Greeting::Hello { version: VERSION, key }).map_err(io::Error::other)?;
        ws.send(Message::text(hello)).map_err(ws_error)?;
        let Message::Binary(first) = next_message(&mut ws, deadline)? else {
            close(ws, UNPAIRED, "expected an encrypted frame");
            return Ok(None);
        };
        let proven = channel.open(&first).ok().and_then(|plain| serde_json::from_slice::<PhoneMsg>(&plain).ok());
        if proven.is_none() {
            close(ws, UNPAIRED, "this phone is not paired with this orkai");
            return Ok(None);
        }
        let (tx, rx) = mpsc::channel();
        let welcome = BridgeMsg::Welcome { host: hub.host().to_string(), version: env!("CARGO_PKG_VERSION").into() };
        let mut phone = Self {
            id: 0,
            ws,
            channel,
            hub: Arc::clone(&hub),
            tx: tx.clone(),
            rx,
            view: None,
            generation: 0,
            heard: Instant::now(),
            pinged: Instant::now(),
        };
        phone.send(&welcome)?;
        phone.id = hub.join(tx);
        Ok(Some(phone))
    }

    fn run(&mut self) -> io::Result<()> {
        loop {
            while let Ok(event) = self.rx.try_recv() {
                self.on_event(event)?;
            }
            self.on_timers()?;
            match self.ws.read() {
                Ok(Message::Binary(frame)) => {
                    self.heard = Instant::now();
                    let plain = self.channel.open(&frame).map_err(io::Error::other)?;
                    match serde_json::from_slice::<PhoneMsg>(&plain) {
                        Ok(message) => self.on_phone(message)?,
                        Err(_) => self.notice(Tone::Warn, "this orkai does not know that action; update it")?,
                    }
                }
                Ok(Message::Ping(_) | Message::Pong(_)) => self.heard = Instant::now(),
                Ok(Message::Close(_)) => return Ok(()),
                Ok(Message::Text(_) | Message::Frame(_)) => return Ok(()),
                Err(err) if is_tick(&err) => {}
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return Ok(()),
                Err(err) => return Err(ws_error(err)),
            }
        }
    }

    fn send(&mut self, message: &BridgeMsg) -> io::Result<()> {
        self.send_plain(&encode(message))
    }

    fn send_plain(&mut self, plain: &[u8]) -> io::Result<()> {
        let frame = self.channel.seal(plain).map_err(io::Error::other)?;
        self.ws.send(Message::binary(frame)).map_err(ws_error)
    }

    fn notice(&mut self, tone: Tone, text: &str) -> io::Result<()> {
        self.send(&BridgeMsg::Notice { text: text.to_string(), tone })
    }

    fn on_timers(&mut self) -> io::Result<()> {
        if self.heard.elapsed() >= SILENCE_LIMIT {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "the phone went quiet"));
        }
        if self.pinged.elapsed() >= PING_EVERY {
            self.pinged = Instant::now();
            self.ws.send(Message::Ping(Vec::new().into())).map_err(ws_error)?;
        }
        let Some(view) = self.view.as_mut() else { return Ok(()) };
        if view.submit_at.is_some_and(|at| Instant::now() >= at) {
            view.submit_at = None;
            if let Some(conn) = view.conn.as_ref() {
                let _ = conn.send(&ClientMsg::Input(SUBMIT.to_vec()));
            }
        }
        if !view.dirty || view.flushed.elapsed() < SCREEN_EVERY {
            return Ok(());
        }
        view.dirty = false;
        view.flushed = Instant::now();
        let update = view.term.as_ref().and_then(|term| {
            let scroll = term.screen().scrollback();
            view.mirror.update(view.agent, term.screen(), scroll)
        });
        match update {
            Some(screen) => self.send(&BridgeMsg::Screen(screen)),
            None => Ok(()),
        }
    }

    fn on_event(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::Broadcast(bytes) => self.send_plain(&bytes),
            Event::Reply(message) => self.send(&message),
            Event::Holder { generation, message } => {
                if self.view.as_ref().is_some_and(|view| view.generation == generation) {
                    self.on_holder(message);
                }
                Ok(())
            }
            Event::Replayed { generation, parser } => {
                if let Some(view) =
                    self.view.as_mut().filter(|view| view.generation == generation && view.conn.is_none())
                {
                    view.term = Some(match parser {
                        Some(parser) => TermView::from_parser(*parser),
                        None => TermView::document(view.want, b"(no output was recorded)"),
                    });
                    view.mirror.reset();
                    view.dirty = true;
                }
                Ok(())
            }
        }
    }

    fn on_holder(&mut self, message: Option<HolderMsg>) {
        let Some(view) = self.view.as_mut() else { return };
        match message {
            Some(HolderMsg::Status(status)) => {
                view.pty = Some(TermSize { rows: status.rows, cols: status.cols });
                view.viewers = status.viewers;
                self.fit();
            }
            Some(HolderMsg::Snapshot(snapshot)) => {
                view.term = Some(TermView::from_snapshot(snapshot.size, &snapshot.data));
                view.mirror.reset();
                view.dirty = true;
            }
            Some(HolderMsg::Output(bytes)) => {
                if let Some(term) = view.term.as_mut() {
                    term.process(&bytes);
                    view.dirty = true;
                }
            }
            Some(HolderMsg::Welcome(_)) => {}
            Some(HolderMsg::Exited(_)) | None => view.conn = None,
        }
    }

    fn fit(&mut self) {
        let Some(view) = self.view.as_mut() else { return };
        let (Some(conn), Some(target)) = (view.conn.as_ref(), view.target()) else { return };
        if view.requested != Some(target) && conn.send(&ClientMsg::Attach(target)).is_ok() {
            view.requested = Some(target);
        }
    }

    fn open(&mut self, agent: AgentId, want: TermSize) {
        self.generation += 1;
        let generation = self.generation;
        let mut view = View {
            agent,
            generation,
            conn: None,
            term: None,
            mirror: Mirror::default(),
            want,
            pty: None,
            viewers: None,
            requested: None,
            dirty: false,
            flushed: Instant::now(),
            submit_at: None,
            wheel: 0,
        };
        let store = self.hub.store().clone();
        let connected = store.read_runtime(agent).ok().flatten().and_then(|runtime| client::connect(&runtime).ok());
        match connected {
            Some((conn, reader, _)) => {
                view.conn = Some(Arc::new(conn));
                let tx = self.tx.clone();
                let _ =
                    client::spawn_reader(reader, move |message| tx.send(Event::Holder { generation, message }).is_ok());
            }
            None => {
                let (tx, path) = (self.tx.clone(), store.home().output_log(agent));
                let _ = thread::Builder::new().name("orkai-pwa-replay".into()).spawn(move || {
                    let parser = oplog::replay(&path).ok().flatten().map(Box::new);
                    let _ = tx.send(Event::Replayed { generation, parser });
                });
            }
        }
        self.view = Some(view);
    }

    fn type_text(&mut self, text: &str) -> io::Result<()> {
        let Some(view) = self.view.as_mut() else { return Ok(()) };
        let Some(conn) = view.conn.clone() else {
            return self.notice(Tone::Warn, "this agent is not running");
        };
        let bracketed = view.term.as_ref().is_some_and(|term| term.screen().bracketed_paste());
        let bytes = encode_typed_text(text, bracketed);
        if bytes.is_empty() {
            let _ = conn.send(&ClientMsg::Input(SUBMIT.to_vec()));
        } else if conn.send(&ClientMsg::Input(bytes)).is_ok() {
            view.submit_at = Some(Instant::now() + SUBMIT_DELAY);
        }
        Ok(())
    }

    fn on_phone(&mut self, message: PhoneMsg) -> io::Result<()> {
        match message {
            PhoneMsg::Sync => self.hub.nudge(),
            PhoneMsg::Open { agent, rows, cols } => self.open(agent, clamp(rows, cols)),
            PhoneMsg::Close => self.view = None,
            PhoneMsg::Size { rows, cols } => {
                if let Some(view) = self.view.as_mut() {
                    view.want = clamp(rows, cols);
                    view.requested = None;
                }
                self.fit();
            }
            PhoneMsg::Keys { data } => {
                if let Some(conn) = self.view.as_ref().and_then(|view| view.conn.as_ref()) {
                    let _ = conn.send(&ClientMsg::Input(data.into_bytes()));
                }
            }
            PhoneMsg::Send { text } => self.type_text(&text)?,
            PhoneMsg::Scroll { lines } => {
                if let Some(view) = self.view.as_mut() {
                    view.scroll(lines);
                }
            }
            other => actions::handle(&self.hub, other, &self.tx),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(viewers: Option<u16>, requested: Option<TermSize>) -> View {
        View {
            agent: 1,
            generation: 1,
            conn: None,
            term: None,
            mirror: Mirror::default(),
            want: TermSize { rows: 30, cols: 50 },
            pty: Some(TermSize { rows: 40, cols: 120 }),
            viewers,
            requested,
            dirty: false,
            flushed: Instant::now(),
            submit_at: None,
            wheel: 0,
        }
    }

    fn showing(output: &[u8]) -> View {
        let mut view = view(Some(0), None);
        let mut term = TermView::new(TermSize { rows: 30, cols: 50 });
        term.process(output);
        view.term = Some(term);
        view
    }

    fn history() -> Vec<u8> {
        (0..40).flat_map(|line| format!("line {line}\r\n").into_bytes()).collect()
    }

    const FULL_SCREEN_APP: &[u8] = b"\x1b[?1049h\x1b[?1002h\x1b[?1006h";
    const TRACKS_MOUSE: &[u8] = b"\x1b[?1002h\x1b[?1006h";
    const UP: &[u8] = b"\x1b[<64;26;16M";
    const DOWN: &[u8] = b"\x1b[<65;26;16M";

    #[test]
    fn a_scroll_on_an_app_that_tracks_the_mouse_becomes_wheel_ticks_in_the_middle_of_its_screen() {
        let mut view = showing(FULL_SCREEN_APP);
        assert_eq!(view.wheel(6), Some([UP, UP].concat()));
        assert_eq!(view.wheel(-3), Some(DOWN.to_vec()));
    }

    #[test]
    fn a_slow_drag_adds_up_to_whole_wheel_ticks() {
        let mut view = showing(FULL_SCREEN_APP);
        assert_eq!(view.wheel(1), Some(Vec::new()));
        assert_eq!(view.wheel(1), Some(Vec::new()));
        assert_eq!(view.wheel(1), Some(UP.to_vec()));
        assert_eq!(view.wheel(-2), Some(Vec::new()));
        assert_eq!(view.wheel(-1), Some(DOWN.to_vec()));
    }

    #[test]
    fn a_huge_scroll_sends_a_bounded_number_of_wheel_ticks() {
        let mut view = showing(FULL_SCREEN_APP);
        let ticks = view.wheel(i32::MAX).unwrap();
        assert_eq!(ticks, UP.repeat(usize::try_from(MAX_WHEEL_LINES / WHEEL_LINES).unwrap()));
        let ticks = view.wheel(i32::MIN).unwrap();
        assert_eq!(ticks, DOWN.repeat(usize::try_from(MAX_WHEEL_LINES / WHEEL_LINES).unwrap()));
    }

    #[test]
    fn an_app_that_ignores_the_mouse_scrolls_the_history_of_the_phone() {
        let mut view = showing(b"one\r\ntwo\r\nthree");
        assert_eq!(view.wheel(3), None);
        let mut view = showing(&history());
        view.scroll(5);
        assert!(view.dirty);
        assert_eq!(view.term.as_ref().unwrap().screen().scrollback(), 5);
        view.scroll(0);
        assert!(!view.term.as_ref().unwrap().is_scrolled());
    }

    #[test]
    fn a_history_already_scrolled_on_the_phone_keeps_scrolling_there() {
        let mut view = showing(&history());
        view.term.as_mut().unwrap().scroll_by(4);
        view.term.as_mut().unwrap().process(TRACKS_MOUSE);
        assert_eq!(view.wheel(3), None, "the phone must first come back down its own history");
    }

    #[test]
    fn an_agent_that_is_not_running_scrolls_what_it_recorded() {
        let mut view = showing(&[history().as_slice(), TRACKS_MOUSE].concat());
        view.scroll(3);
        assert_eq!(view.term.as_ref().unwrap().screen().scrollback(), 3);
    }

    #[test]
    fn a_phone_alone_on_an_agent_gets_the_size_of_its_screen() {
        assert_eq!(view(Some(0), None).target(), Some(TermSize { rows: 30, cols: 50 }));
        let attached = Some(TermSize { rows: 30, cols: 50 });
        assert_eq!(view(Some(1), attached).target(), Some(TermSize { rows: 30, cols: 50 }));
    }

    #[test]
    fn a_phone_gets_the_size_of_its_screen_even_when_the_desk_watches_too() {
        assert_eq!(view(Some(1), None).target(), Some(TermSize { rows: 30, cols: 50 }));
        let attached = Some(TermSize { rows: 30, cols: 50 });
        assert_eq!(view(Some(2), attached).target(), Some(TermSize { rows: 30, cols: 50 }));
    }

    #[test]
    fn an_older_holder_that_does_not_count_viewers_is_never_resized() {
        assert_eq!(view(None, None).target(), Some(TermSize { rows: 40, cols: 120 }));
    }

    #[test]
    fn nothing_is_attached_before_the_holder_reports_its_size() {
        let mut waiting = view(Some(0), None);
        waiting.pty = None;
        assert_eq!(waiting.target(), None);
    }

    #[test]
    fn phone_sizes_are_kept_in_a_sane_range() {
        assert_eq!(clamp(1, 1), MIN_SIZE);
        assert_eq!(clamp(9_999, 9_999), MAX_SIZE);
        assert_eq!(clamp(30, 50), TermSize { rows: 30, cols: 50 });
    }
}
