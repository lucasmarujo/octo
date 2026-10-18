pub mod detect;
pub mod input;
pub mod nested;
pub mod oplog;
pub mod term;

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::fsutil;
use crate::hooks::HookInbox;
use crate::launch;
use crate::model::{AgentId, AgentKind, ExitInfo, LaunchSpec, RuntimeInfo, StatusReport};
use crate::paths::Home;
use crate::protocol::{ClientMsg, HolderMsg, PROTOCOL_VERSION, Snapshot, TermSize, Welcome};
use crate::util::{constant_time_eq, lock, now_ms, random_token};

use oplog::OutputLog;
use term::TermState;

const TICK: Duration = Duration::from_millis(200);
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_QUEUE: usize = 4_096;
const EXIT_LINGER: Duration = Duration::from_millis(400);
const LOG_FLUSH_EVERY_TICKS: u32 = 5;

type Frame = Arc<Vec<u8>>;

struct Watch {
    root: u32,
    claude_home: Option<PathBuf>,
}

struct ClientSlot {
    id: u64,
    tx: SyncSender<Frame>,
    attached: bool,
}

#[derive(Default)]
struct Clients {
    slots: Vec<ClientSlot>,
}

impl Clients {
    fn broadcast(&mut self, frame: &Frame, attached_only: bool) {
        self.slots.retain(|slot| {
            if attached_only && !slot.attached {
                return true;
            }
            match slot.tx.try_send(Arc::clone(frame)) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
            }
        });
    }

    fn send_to(&mut self, id: u64, frame: Frame) {
        if let Some(index) = self.slots.iter().position(|s| s.id == id)
            && self.slots[index].tx.try_send(frame).is_err()
        {
            self.slots.remove(index);
        }
    }

    fn set_attached(&mut self, id: u64, attached: bool) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.id == id) {
            slot.attached = attached;
        }
    }

    fn remove(&mut self, id: u64) {
        self.slots.retain(|s| s.id != id);
    }
}

struct Shared {
    home: Home,
    agent_id: AgentId,
    token: String,
    term: Mutex<TermState>,
    pty_writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    clients: Mutex<Clients>,
    log: Mutex<Option<OutputLog>>,
    exit: Mutex<Option<ExitInfo>>,
    next_client: AtomicU64,
    finished: AtomicBool,
}

impl Shared {
    fn diag(&self, message: &str) {
        diag(&self.home, self.agent_id, message);
    }

    fn status(&self) -> StatusReport {
        let exit = lock(&self.exit).clone();
        lock(&self.term).report(exit)
    }

    fn write_pty(&self, bytes: &[u8]) {
        let mut writer = lock(&self.pty_writer);
        if let Err(err) = writer.write_all(bytes).and_then(|()| writer.flush()) {
            drop(writer);
            self.diag(&format!("pty write failed: {err}"));
        }
    }

    fn resize(&self, term: &mut TermState, size: TermSize) {
        if term.size() == size {
            return;
        }
        term.resize(size);
        let pty_size = PtySize { rows: size.rows, cols: size.cols, pixel_width: 0, pixel_height: 0 };
        if let Err(err) = lock(&self.master).resize(pty_size) {
            self.diag(&format!("pty resize failed: {err}"));
        }
        if let Some(log) = lock(&self.log).as_mut() {
            let _ = log.resize(size);
        }
    }

    fn on_output(&self, chunk: &[u8]) {
        let now = now_ms();
        let replies = {
            let mut term = lock(&self.term);
            let replies = term.process(chunk, now);
            let frame = Arc::new(HolderMsg::Output(chunk.to_vec()).to_frame());
            lock(&self.clients).broadcast(&frame, true);
            replies
        };
        if !replies.is_empty() {
            self.write_pty(&replies);
        }
        if let Some(log) = lock(&self.log).as_mut()
            && let Err(err) = log.output(chunk)
        {
            self.diag(&format!("log write failed: {err}"));
        }
    }

    fn attach(&self, client: u64, size: TermSize) {
        let mut term = lock(&self.term);
        self.resize(&mut term, size);
        let data = term.snapshot();
        let seen_changed = term.mark_seen();
        let frame = Arc::new(HolderMsg::Snapshot(Snapshot { size: term.size(), data }).to_frame());
        let mut clients = lock(&self.clients);
        clients.send_to(client, frame);
        clients.set_attached(client, true);
        drop(clients);
        drop(term);
        if seen_changed {
            self.broadcast_status();
        }
    }

    fn broadcast_status(&self) {
        let frame = Arc::new(HolderMsg::Status(self.status()).to_frame());
        lock(&self.clients).broadcast(&frame, false);
    }

    fn handle(&self, client: u64, msg: ClientMsg) {
        match msg {
            ClientMsg::Hello(_) => {}
            ClientMsg::Attach(size) => self.attach(client, TermSize::new(size.rows, size.cols)),
            ClientMsg::Detach => lock(&self.clients).set_attached(client, false),
            ClientMsg::Input(bytes) => {
                lock(&self.term).note_input(&bytes);
                self.write_pty(&bytes);
            }
            ClientMsg::Resize(size) => {
                let mut term = lock(&self.term);
                self.resize(&mut term, TermSize::new(size.rows, size.cols));
            }
            ClientMsg::Kill => {
                if let Err(err) = lock(&self.killer).kill() {
                    self.diag(&format!("kill failed: {err}"));
                }
            }
            ClientMsg::Seen => {
                if lock(&self.term).mark_seen() {
                    self.broadcast_status();
                }
            }
        }
    }
}

pub fn diag(home: &Home, id: AgentId, message: &str) {
    let line = format!("{} {}\n", now_ms(), message);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(home.holder_log(id)) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn record_failure(home: &Home, id: AgentId, error: String) -> io::Result<()> {
    diag(home, id, &error);
    fsutil::write_json_atomic(&home.exit_file(id), &ExitInfo { code: None, error: Some(error), at_ms: now_ms() })
}

fn build_command(spec: &LaunchSpec) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    cmd.cwd(&spec.cwd);
    for key in &spec.env_remove {
        cmd.env_remove(key);
    }
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    cmd
}

pub fn run(home: &Home, id: AgentId) -> io::Result<()> {
    let spec: LaunchSpec = fsutil::read_json(&home.launch_file(id))?;
    fsutil::remove_file_if_exists(&home.exit_file(id))?;
    let started_at = now_ms();
    let size = TermSize::new(spec.rows, spec.cols);
    let pty = match native_pty_system().openpty(PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(err) => return record_failure(home, id, format!("could not open a pseudo terminal: {err}")),
    };
    let child = match pty.slave.spawn_command(build_command(&spec)) {
        Ok(child) => child,
        Err(err) => {
            return record_failure(home, id, format!("could not start {}: {err}", spec.program.display()));
        }
    };
    drop(pty.slave);
    let mut child = child;
    let reader = pty.master.try_clone_reader().map_err(io::Error::other)?;
    let writer = pty.master.take_writer().map_err(io::Error::other)?;
    let killer = child.clone_killer();
    let child_pid = child.process_id();

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let token = random_token()?;

    let log = match OutputLog::open(home.output_log(id), size) {
        Ok(log) => Some(log),
        Err(err) => {
            diag(home, id, &format!("output log unavailable: {err}"));
            None
        }
    };

    let shared = Arc::new(Shared {
        home: home.clone(),
        agent_id: id,
        token: token.clone(),
        term: Mutex::new(TermState::new(spec.kind, size, started_at, task_of(&spec), spec.initial_input.clone())),
        pty_writer: Mutex::new(writer),
        master: Mutex::new(pty.master),
        killer: Mutex::new(killer),
        clients: Mutex::new(Clients::default()),
        log: Mutex::new(log),
        exit: Mutex::new(None),
        next_client: AtomicU64::new(1),
        finished: AtomicBool::new(false),
    });

    fsutil::write_json_atomic(
        &home.runtime_file(id),
        &RuntimeInfo {
            holder_pid: std::process::id(),
            child_pid,
            port,
            token,
            started_at_ms: started_at,
            protocol: PROTOCOL_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            exe: std::env::current_exe().ok(),
        },
    )?;

    spawn_named("orkai-pty-reader", {
        let shared = Arc::clone(&shared);
        move || pump_output(&shared, reader)
    })?;
    spawn_named("orkai-acceptor", {
        let shared = Arc::clone(&shared);
        move || accept_clients(&shared, listener)
    })?;
    let watch = child_pid
        .filter(|_| spec.kind == AgentKind::Terminal)
        .map(|root| Watch { root, claude_home: launch::claude_home() });
    let inbox = spec.kind.is_ai().then(|| HookInbox::open(home.hooks_dir(id)));
    spawn_named("orkai-ticker", {
        let shared = Arc::clone(&shared);
        move || tick(&shared, watch.as_ref(), inbox)
    })?;

    let status = child.wait();
    thread::sleep(Duration::from_millis(150));
    let info = ExitInfo {
        code: status.as_ref().ok().map(|s| s.exit_code()),
        error: status.err().map(|e| e.to_string()),
        at_ms: now_ms(),
    };
    finish(&shared, info);
    Ok(())
}

fn task_of(spec: &LaunchSpec) -> String {
    match spec.kind {
        AgentKind::Terminal => String::new(),
        _ => spec.initial_input.clone().unwrap_or_default(),
    }
}

fn finish(shared: &Arc<Shared>, info: ExitInfo) {
    shared.finished.store(true, Ordering::SeqCst);
    *lock(&shared.exit) = Some(info.clone());
    if let Some(log) = lock(&shared.log).as_mut() {
        let _ = log.flush();
    }
    if let Err(err) = fsutil::write_json_atomic(&shared.home.exit_file(shared.agent_id), &info) {
        shared.diag(&format!("could not record exit: {err}"));
    }
    shared.broadcast_status();
    let frame = Arc::new(HolderMsg::Exited(info).to_frame());
    lock(&shared.clients).broadcast(&frame, false);
    thread::sleep(EXIT_LINGER);
    let _ = fsutil::remove_file_if_exists(&shared.home.runtime_file(shared.agent_id));
}

fn spawn_named<F>(name: &str, f: F) -> io::Result<()>
where
    F: FnOnce() + Send + 'static,
{
    thread::Builder::new().name(name.to_string()).spawn(f).map(|_| ())
}

fn pump_output(shared: &Arc<Shared>, mut reader: Box<dyn Read + Send>) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => shared.on_output(&buf[..n]),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn tick(shared: &Arc<Shared>, watch: Option<&Watch>, mut inbox: Option<HookInbox>) {
    let mut last: Option<StatusReport> = None;
    let mut ticks: u32 = 0;
    while !shared.finished.load(Ordering::SeqCst) {
        thread::sleep(TICK);
        ticks = ticks.wrapping_add(1);
        let now = now_ms();
        if let Some(watch) = watch
            && ticks.is_multiple_of(nested::CHECK_EVERY_TICKS)
        {
            let found = nested::detect(&nested::descendants(watch.root), watch.claude_home.as_deref(), now);
            lock(&shared.term).set_nested(found, now);
        }
        let events = inbox.as_mut().map(HookInbox::take).unwrap_or_default();
        let (typed, submit) = {
            let mut term = lock(&shared.term);
            for event in events {
                term.apply_hook(event.signal, event.at_ms);
            }
            term.evaluate(now);
            (term.take_ready_input(now), term.take_submit(now))
        };
        if let Some(bytes) = typed {
            shared.write_pty(&bytes);
        }
        if submit {
            shared.write_pty(b"\r");
        }
        let report = shared.status();
        if last.as_ref() != Some(&report) {
            let frame = Arc::new(HolderMsg::Status(report.clone()).to_frame());
            lock(&shared.clients).broadcast(&frame, false);
            last = Some(report);
        }
        if ticks.is_multiple_of(LOG_FLUSH_EVERY_TICKS)
            && let Some(log) = lock(&shared.log).as_mut()
        {
            let _ = log.flush();
        }
    }
}

fn accept_clients(shared: &Arc<Shared>, listener: TcpListener) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let shared = Arc::clone(shared);
        let _ = spawn_named("orkai-client", move || {
            if let Err(err) = serve_client(&shared, stream)
                && err.kind() != io::ErrorKind::UnexpectedEof
            {
                shared.diag(&format!("client error: {err}"));
            }
        });
    }
}

fn serve_client(shared: &Arc<Shared>, mut stream: TcpStream) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HELLO_TIMEOUT))?;
    let hello = match ClientMsg::read(&mut stream)? {
        ClientMsg::Hello(hello) => hello,
        _ => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "expected hello")),
    };
    if !constant_time_eq(hello.token.as_bytes(), shared.token.as_bytes()) {
        let _ = stream.shutdown(Shutdown::Both);
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "invalid token"));
    }
    stream.set_read_timeout(None)?;

    let id = shared.next_client.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::sync_channel::<Frame>(CLIENT_QUEUE);
    let mut write_stream = stream.try_clone()?;
    spawn_named("orkai-client-writer", move || {
        for frame in rx {
            if write_stream.write_all(&frame).is_err() {
                break;
            }
        }
        let _ = write_stream.shutdown(Shutdown::Both);
    })?;

    let welcome = HolderMsg::Welcome(Welcome {
        agent_id: shared.agent_id,
        protocol: PROTOCOL_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        holder_pid: std::process::id(),
    });
    let _ = tx.try_send(Arc::new(welcome.to_frame()));
    let _ = tx.try_send(Arc::new(HolderMsg::Status(shared.status()).to_frame()));
    lock(&shared.clients).slots.push(ClientSlot { id, tx, attached: false });

    let result = loop {
        match ClientMsg::read(&mut stream) {
            Ok(msg) => shared.handle(id, msg),
            Err(err) => break err,
        }
    };
    lock(&shared.clients).remove(id);
    let _ = stream.shutdown(Shutdown::Both);
    if result.kind() == io::ErrorKind::UnexpectedEof || result.kind() == io::ErrorKind::ConnectionReset {
        Ok(())
    } else {
        Err(result)
    }
}
