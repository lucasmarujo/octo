use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use orkai::client::{self, Connection};
use orkai::holder::oplog;
use orkai::model::AgentId;
use orkai::protocol::{ClientMsg, HolderMsg, TermSize};
use orkai::termfmt;
use orkai::util::lock;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use crate::link::Link;

pub const OUTPUT: u8 = 0;
pub const SNAPSHOT: u8 = 1;
pub const ENDED: u8 = 2;
const NOTHING_RECORDED: &[u8] = b"(no output was recorded)";

pub type SessionId = u64;

#[derive(Default)]
pub struct Console {
    session: Mutex<Option<(SessionId, Arc<Connection>)>>,
    next: AtomicU64,
}

pub fn frame(message: &HolderMsg) -> Option<Vec<u8>> {
    match message {
        HolderMsg::Output(bytes) => Some([&[OUTPUT][..], bytes].concat()),
        HolderMsg::Snapshot(snapshot) => Some(snapshot_frame(snapshot.size, &snapshot.data)),
        HolderMsg::Exited(_) => Some(vec![ENDED]),
        HolderMsg::Welcome(_) | HolderMsg::Status(_) => None,
    }
}

fn snapshot_frame(size: TermSize, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 5);
    out.push(SNAPSHOT);
    out.extend_from_slice(&size.rows.to_le_bytes());
    out.extend_from_slice(&size.cols.to_le_bytes());
    out.extend_from_slice(data);
    out
}

pub fn recorded(log: &Path, fallback: TermSize) -> Vec<u8> {
    match oplog::replay(log).ok().flatten() {
        Some(mut parser) => {
            let (rows, cols) = parser.screen().size();
            snapshot_frame(TermSize::new(rows, cols), &termfmt::snapshot(parser.screen_mut()))
        }
        None => snapshot_frame(fallback, NOTHING_RECORDED),
    }
}

fn push(channel: &Channel, bytes: Vec<u8>) -> bool {
    channel.send(InvokeResponseBody::Raw(bytes)).is_ok()
}

impl Console {
    pub fn open(&self, link: &Link, agent: AgentId, size: TermSize, channel: Channel) -> Result<SessionId, String> {
        self.close_current();
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let store = link.hub().store();
        let connected = store.read_runtime(agent).ok().flatten().and_then(|runtime| client::connect(&runtime).ok());
        let Some((conn, reader, _)) = connected else {
            push(&channel, recorded(&store.home().output_log(agent), size));
            push(&channel, vec![ENDED]);
            return Ok(id);
        };
        conn.send(&ClientMsg::Attach(size)).map_err(|err| err.to_string())?;
        client::spawn_reader(reader, move |message| match message {
            Some(message) => frame(&message).is_none_or(|bytes| push(&channel, bytes)),
            None => {
                push(&channel, vec![ENDED]);
                false
            }
        })
        .map_err(|err| err.to_string())?;
        *lock(&self.session) = Some((id, Arc::new(conn)));
        Ok(id)
    }

    fn deliver(&self, session: SessionId, message: ClientMsg) -> Result<(), String> {
        let conn = lock(&self.session)
            .as_ref()
            .filter(|(current, _)| *current == session)
            .map(|(_, conn)| Arc::clone(conn))
            .ok_or_else(|| "this agent terminal is not open anymore".to_string())?;
        conn.send(&message).map_err(|err| err.to_string())
    }

    pub fn input(&self, session: SessionId, data: Vec<u8>) -> Result<(), String> {
        self.deliver(session, ClientMsg::Input(data))
    }

    pub fn resize(&self, session: SessionId, size: TermSize) -> Result<(), String> {
        self.deliver(session, ClientMsg::Resize(size))
    }

    pub fn close(&self, session: SessionId) {
        let mut current = lock(&self.session);
        if current.as_ref().is_some_and(|(id, _)| *id == session) {
            Self::detach(current.take());
        }
    }

    fn close_current(&self) {
        Self::detach(lock(&self.session).take());
    }

    fn detach(session: Option<(SessionId, Arc<Connection>)>) {
        if let Some((_, conn)) = session {
            let _ = conn.send(&ClientMsg::Detach);
            conn.close();
        }
    }
}

#[tauri::command(async)]
pub fn term_open(
    link: State<'_, Link>,
    console: State<'_, Console>,
    agent: AgentId,
    rows: u16,
    cols: u16,
    channel: Channel,
) -> Result<SessionId, String> {
    console.open(&link, agent, TermSize::new(rows, cols), channel)
}

#[tauri::command]
pub fn term_input(console: State<'_, Console>, session: SessionId, data: Vec<u8>) -> Result<(), String> {
    console.input(session, data)
}

#[tauri::command]
pub fn term_resize(console: State<'_, Console>, session: SessionId, rows: u16, cols: u16) -> Result<(), String> {
    console.resize(session, TermSize::new(rows, cols))
}

#[tauri::command]
pub fn term_close(console: State<'_, Console>, session: SessionId) {
    console.close(session);
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::Duration;

    use orkai::config::Config;
    use orkai::launch::Launcher;
    use orkai::model::RuntimeInfo;
    use orkai::paths::Home;
    use orkai::protocol::{PROTOCOL_VERSION, Snapshot, Welcome};
    use orkai::pwa::hub::Hub;
    use orkai::store::Store;

    use super::*;

    const AGENT: AgentId = 7;
    const WAIT: Duration = Duration::from_secs(10);

    struct Fixture {
        dir: std::path::PathBuf,
        link: Link,
        home: Home,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(name: &str) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("octo-term-{name}-{}-{}", std::process::id(), orkai::util::now_ms()));
        let home = Home::at(dir.join("home"));
        let store = Store::open(home.clone()).unwrap();
        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        Fixture { link: Link::new(Hub::new(launcher, "testhost".into())), home, dir }
    }

    fn window() -> (Channel, Receiver<Vec<u8>>) {
        let (tx, rx) = mpsc::channel();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                let _ = tx.send(bytes);
            }
            Ok(())
        });
        (channel, rx)
    }

    fn reply(stream: &mut TcpStream, message: HolderMsg) {
        stream.write_all(&message.to_frame()).unwrap();
    }

    fn fake_holder(home: &Home) -> Receiver<TcpStream> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let runtime = RuntimeInfo {
            holder_pid: std::process::id(),
            child_pid: None,
            port: listener.local_addr().unwrap().port(),
            token: "secret".into(),
            started_at_ms: orkai::util::now_ms(),
            protocol: PROTOCOL_VERSION,
            version: "test".into(),
            exe: None,
        };
        let path = home.runtime_file(AGENT);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        orkai::fsutil::write_json_atomic(&path, &runtime).unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let ClientMsg::Hello(hello) = ClientMsg::read(&mut stream).unwrap() else { panic!("no hello") };
            assert_eq!(hello.token, "secret");
            let welcome =
                Welcome { agent_id: AGENT, protocol: PROTOCOL_VERSION, version: "test".into(), holder_pid: 1 };
            reply(&mut stream, HolderMsg::Welcome(welcome));
            tx.send(stream).unwrap();
        });
        rx
    }

    #[test]
    fn holder_messages_become_one_byte_tagged_frames() {
        assert_eq!(frame(&HolderMsg::Output(b"hi".to_vec())), Some(vec![OUTPUT, b'h', b'i']));
        let snapshot = HolderMsg::Snapshot(Snapshot { size: TermSize::new(30, 300), data: b"ab".to_vec() });
        assert_eq!(frame(&snapshot), Some(vec![SNAPSHOT, 30, 0, 44, 1, b'a', b'b']));
        let exited = HolderMsg::Exited(orkai::model::ExitInfo { code: Some(0), error: None, at_ms: 1 });
        assert_eq!(frame(&exited), Some(vec![ENDED]));
    }

    #[test]
    fn the_terminal_talks_to_the_holder_directly_in_raw_bytes() {
        let fx = fixture("live");
        let holder = fake_holder(&fx.home);
        let console = Console::default();
        let (channel, frames) = window();
        let session = console.open(&fx.link, AGENT, TermSize::new(40, 120), channel).unwrap();
        let mut stream = holder.recv_timeout(WAIT).unwrap();
        assert_eq!(ClientMsg::read(&mut stream).unwrap(), ClientMsg::Attach(TermSize::new(40, 120)));

        reply(
            &mut stream,
            HolderMsg::Snapshot(Snapshot { size: TermSize::new(40, 120), data: b"\x1b[1mready".to_vec() }),
        );
        let snapshot = frames.recv_timeout(WAIT).unwrap();
        assert_eq!(&snapshot[..5], &[SNAPSHOT, 40, 0, 120, 0]);
        assert_eq!(&snapshot[5..], b"\x1b[1mready");
        reply(&mut stream, HolderMsg::Output("ção\r\n".as_bytes().to_vec()));
        assert_eq!(frames.recv_timeout(WAIT).unwrap(), [&[OUTPUT][..], "ção\r\n".as_bytes()].concat());

        console.input(session, b"ls\r".to_vec()).unwrap();
        assert_eq!(ClientMsg::read(&mut stream).unwrap(), ClientMsg::Input(b"ls\r".to_vec()));
        console.resize(session, TermSize::new(50, 160)).unwrap();
        assert_eq!(ClientMsg::read(&mut stream).unwrap(), ClientMsg::Resize(TermSize::new(50, 160)));

        drop(stream);
        assert_eq!(frames.recv_timeout(WAIT).unwrap(), vec![ENDED], "the window learns the holder is gone");
        console.close(session);
        assert!(console.input(session, b"x".to_vec()).unwrap_err().contains("not open anymore"));
    }

    #[test]
    fn a_terminal_that_was_replaced_cannot_close_or_type_into_the_new_one() {
        let fx = fixture("replaced");
        let holder = fake_holder(&fx.home);
        let console = Console::default();
        let (grid, _grid_frames) = window();
        let first = console.open(&fx.link, AGENT, TermSize::new(20, 60), grid).unwrap();
        let mut first_stream = holder.recv_timeout(WAIT).unwrap();
        assert!(matches!(ClientMsg::read(&mut first_stream).unwrap(), ClientMsg::Attach(_)));

        let holder = fake_holder(&fx.home);
        let (tab, _tab_frames) = window();
        let second = console.open(&fx.link, AGENT, TermSize::new(40, 120), tab).unwrap();
        assert_ne!(first, second);
        assert_eq!(ClientMsg::read(&mut first_stream).unwrap(), ClientMsg::Detach, "opening the tab lets the grid go");
        let mut second_stream = holder.recv_timeout(WAIT).unwrap();
        assert!(matches!(ClientMsg::read(&mut second_stream).unwrap(), ClientMsg::Attach(_)));

        console.close(first);
        assert!(console.input(first, b"stale".to_vec()).is_err());
        console.input(second, b"live".to_vec()).unwrap();
        assert_eq!(
            ClientMsg::read(&mut second_stream).unwrap(),
            ClientMsg::Input(b"live".to_vec()),
            "the tab is still open"
        );
    }

    #[test]
    fn closing_detaches_so_the_holder_hands_the_size_back() {
        let fx = fixture("close");
        let holder = fake_holder(&fx.home);
        let console = Console::default();
        let (channel, _frames) = window();
        let session = console.open(&fx.link, AGENT, TermSize::new(24, 80), channel).unwrap();
        let mut stream = holder.recv_timeout(WAIT).unwrap();
        assert!(matches!(ClientMsg::read(&mut stream).unwrap(), ClientMsg::Attach(_)));
        console.close(session);
        assert_eq!(ClientMsg::read(&mut stream).unwrap(), ClientMsg::Detach);
        assert!(ClientMsg::read(&mut stream).is_err(), "the connection is closed after the detach");
    }

    #[test]
    fn an_agent_without_a_holder_shows_what_it_recorded_and_ends() {
        let fx = fixture("recorded");
        let log = fx.home.output_log(AGENT);
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        let mut writer = oplog::OutputLog::open(log, TermSize::new(6, 20)).unwrap();
        writer.output(b"first line\r\nsecond line").unwrap();
        writer.flush().unwrap();
        drop(writer);
        let console = Console::default();
        let (channel, frames) = window();
        console.open(&fx.link, AGENT, TermSize::new(30, 100), channel).unwrap();
        let snapshot = frames.recv_timeout(WAIT).unwrap();
        assert_eq!(&snapshot[..5], &[SNAPSHOT, 6, 0, 20, 0], "the recorded size, not the window's");
        let text = String::from_utf8_lossy(&snapshot[5..]);
        assert!(text.contains("first line") && text.contains("second line"), "{text}");
        assert_eq!(frames.recv_timeout(WAIT).unwrap(), vec![ENDED]);
    }

    #[test]
    fn an_agent_that_never_wrote_says_so() {
        let fx = fixture("empty");
        let console = Console::default();
        let (channel, frames) = window();
        console.open(&fx.link, AGENT, TermSize::new(30, 100), channel).unwrap();
        let snapshot = frames.recv_timeout(WAIT).unwrap();
        assert_eq!(&snapshot[..5], &[SNAPSHOT, 30, 0, 100, 0]);
        assert_eq!(&snapshot[5..], NOTHING_RECORDED);
        assert_eq!(frames.recv_timeout(WAIT).unwrap(), vec![ENDED]);
    }
}
