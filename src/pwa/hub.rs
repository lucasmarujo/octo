use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::style::Color;

use crate::client::{self, Connection};
use crate::config::Config;
use crate::launch::{self, Launcher};
use crate::model::{
    AgentId, AgentKind, AgentRecord, AgentState, ExitInfo, RuntimeInfo, Session, StatusReport, UiState,
};
use crate::paths::Home;
use crate::protocol::{ClientMsg, HolderMsg};
use crate::pwa::protocol::{AgentView, BridgeMsg, Overview, PhoneMsg, SessionView, UsageEntry, UsageWindow};
use crate::store::Store;
use crate::tui::format::{display_path, project_label};
use crate::tui::render::{USAGE_MONTHLY_LABEL, USAGE_SESSION_LABEL, USAGE_WEEKLY_LABEL};
use crate::tui::theme;
use crate::usage::{self, Limits, Problem};
use crate::util::{lock, now_ms};

pub const WATCH_EVERY: Duration = Duration::from_millis(500);
const RESCAN_EVERY: Duration = Duration::from_secs(1);
const FRESH_RECORD_MS: u64 = 15_000;
const STARTING: &str = "starting…";
const INTERRUPTED: &str = "interrupted";

pub enum Event {
    Broadcast(Arc<Vec<u8>>),
    Reply(BridgeMsg),
    Holder { generation: u64, message: Option<HolderMsg> },
    Replayed { generation: u64, parser: Option<Box<vt100::Parser>> },
    Inbound(PhoneMsg),
}

enum Link {
    Pending,
    Connecting,
    Live(Arc<Connection>),
    Exited(ExitInfo),
    Lost(Option<u64>),
}

struct Tracked {
    record: AgentRecord,
    link: Link,
    status: Option<StatusReport>,
    generation: u64,
}

impl Tracked {
    fn is_dead(&self) -> bool {
        matches!(self.link, Link::Exited(_) | Link::Lost(_))
    }

    fn state(&self) -> AgentState {
        match self.link {
            Link::Exited(_) => AgentState::Exited,
            Link::Lost(_) => AgentState::Lost,
            _ => self.status.as_ref().map_or(AgentState::Idle, |status| status.state),
        }
    }

    fn view(&self) -> AgentView {
        let live = self.status.as_ref().filter(|_| !self.is_dead());
        let starting = matches!(self.link, Link::Pending | Link::Connecting) && self.status.is_none();
        let activity = match &self.link {
            _ if starting => STARTING.to_string(),
            Link::Lost(_) => INTERRUPTED.to_string(),
            Link::Exited(info) => info.describe(),
            _ => live
                .map(|status| status.activity.clone())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| self.record.task.split_whitespace().collect::<Vec<_>>().join(" ")),
        };
        let project = project_label(self.record.project_dir());
        AgentView {
            id: self.record.id,
            session: self.record.session.clone(),
            name: self.record.name.clone(),
            kind: live.map_or(self.record.kind, |status| status.running_kind(self.record.kind)),
            state: self.state(),
            starting,
            activity,
            task: self.record.task.clone(),
            cwd: self.record.cwd.to_string_lossy().into_owned(),
            color: hex(theme::project_color(&project)),
            project,
            started_at: live.map(|status| status.started_at_ms),
            state_since: live.map(|status| status.state_since_ms),
            turn_started: live.and_then(|status| status.turn_started_ms),
            last_turn: live.and_then(|status| status.last_turn_ms),
            branch: self.record.worktree.as_ref().map(|worktree| worktree.branch.clone()),
            exit: match &self.link {
                Link::Exited(info) => Some(info.describe()),
                _ => None,
            },
        }
    }
}

pub fn hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#9696a0".to_string(),
    }
}

#[derive(Default)]
struct Inner {
    phones: BTreeMap<u64, Sender<Event>>,
    next_phone: u64,
    agents: BTreeMap<AgentId, Tracked>,
    sessions: Vec<Session>,
    ui: UiState,
    published: Option<Overview>,
    usage: Option<Vec<UsageEntry>>,
    usage_at: Option<Instant>,
    usage_busy: bool,
    watching: bool,
    due: bool,
}

pub struct Hub {
    store: Store,
    launcher: Launcher,
    host: String,
    kinds: Vec<AgentKind>,
    inner: Mutex<Inner>,
    wake: Condvar,
}

enum Check {
    Exited(ExitInfo),
    Running(RuntimeInfo),
    Absent,
}

impl Hub {
    pub fn new(launcher: Launcher, host: String) -> Arc<Self> {
        let kinds = launch::installed_kinds(launcher.config());
        Arc::new(Self {
            store: launcher.store().clone(),
            launcher,
            host,
            kinds,
            inner: Mutex::new(Inner::default()),
            wake: Condvar::new(),
        })
    }

    pub fn open(home: &Home) -> io::Result<Arc<Self>> {
        let store = Store::open(home.clone())?;
        let config = Config::load_or_init(home)?;
        let launcher = Launcher::new(store, config, &std::env::current_exe()?)?;
        let host = gethostname::gethostname().to_string_lossy().to_lowercase();
        Ok(Self::new(launcher, host))
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn launcher(&self) -> &Launcher {
        &self.launcher
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn kinds(&self) -> &[AgentKind] {
        &self.kinds
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        lock(&self.inner)
    }

    pub fn join(self: &Arc<Self>, tx: Sender<Event>) -> u64 {
        let mut inner = self.inner();
        inner.next_phone += 1;
        let id = inner.next_phone;
        if let Some(overview) = inner.published.clone() {
            let _ = tx.send(Event::Broadcast(Arc::new(encode(&stamped(overview)))));
        }
        if let Some(entries) = inner.usage.clone() {
            let _ = tx.send(Event::Broadcast(Arc::new(encode(&BridgeMsg::Usage { entries }))));
        }
        inner.phones.insert(id, tx);
        inner.due = true;
        if !inner.watching {
            inner.watching = true;
            let hub = Arc::clone(self);
            let spawned = thread::Builder::new().name("orkai-pwa-watch".into()).spawn(move || hub.watch());
            if spawned.is_err() {
                inner.watching = false;
            }
        }
        drop(inner);
        self.wake.notify_all();
        id
    }

    pub fn leave(&self, id: u64) {
        self.inner().phones.remove(&id);
        self.wake.notify_all();
    }

    pub fn phones(&self) -> usize {
        self.inner().phones.len()
    }

    pub fn nudge(&self) {
        self.inner().due = true;
        self.wake.notify_all();
    }

    pub fn record(&self, id: AgentId) -> Option<AgentRecord> {
        self.inner().agents.get(&id).map(|tracked| tracked.record.clone())
    }

    pub fn state(&self, id: AgentId) -> Option<AgentState> {
        self.inner().agents.get(&id).map(Tracked::state)
    }

    pub fn lost_in(&self, session: &str) -> Vec<AgentRecord> {
        self.inner()
            .agents
            .values()
            .filter(|tracked| tracked.record.session == session && matches!(tracked.link, Link::Lost(_)))
            .map(|tracked| tracked.record.clone())
            .collect()
    }

    pub fn name_taken(&self, session: &str, name: &str) -> bool {
        self.inner().agents.values().any(|tracked| tracked.record.session == session && tracked.record.name == name)
    }

    pub fn send(&self, id: AgentId, message: &ClientMsg) -> io::Result<()> {
        let live = self.inner().agents.get(&id).and_then(|tracked| match &tracked.link {
            Link::Live(conn) => Some(Arc::clone(conn)),
            _ => None,
        });
        match live {
            Some(conn) => conn.send(message),
            None => {
                let runtime = self
                    .store
                    .read_runtime(id)?
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "the agent is not running"))?;
                client::connect(&runtime)?.0.send(message)
            }
        }
    }

    pub fn broadcast(&self, message: &BridgeMsg) {
        let bytes = Arc::new(encode(message));
        self.inner().phones.retain(|_, tx| tx.send(Event::Broadcast(Arc::clone(&bytes))).is_ok());
    }

    fn watch(self: Arc<Self>) {
        let mut rescanned: Option<Instant> = None;
        loop {
            {
                let mut inner = self.inner();
                if inner.phones.is_empty() {
                    inner.watching = false;
                    inner.agents.clear();
                    inner.published = None;
                    return;
                }
            }
            let due = std::mem::take(&mut self.inner().due);
            if due || rescanned.is_none_or(|at| at.elapsed() >= RESCAN_EVERY) {
                self.rescan();
                rescanned = Some(Instant::now());
            }
            self.publish();
            self.refresh_usage();
            let inner = self.inner();
            let _ = self.wake.wait_timeout_while(inner, WATCH_EVERY, |inner| !inner.due && !inner.phones.is_empty());
        }
    }

    fn rescan(self: &Arc<Self>) {
        let Ok(records) = self.store.list_agents() else { return };
        let sessions = self.store.peek_sessions_with(&records).map(|(sessions, _)| sessions).unwrap_or_default();
        let ui = self.store.load_ui();
        let unsettled: Vec<AgentId> = {
            let inner = self.inner();
            records
                .iter()
                .filter(|record| {
                    inner.agents.get(&record.id).is_none_or(|t| !matches!(t.link, Link::Live(_) | Link::Connecting))
                })
                .map(|record| record.id)
                .collect()
        };
        let checks: BTreeMap<AgentId, Check> = unsettled
            .into_iter()
            .map(|id| {
                let check = match (self.store.read_exit(id), self.store.read_runtime(id)) {
                    (Ok(Some(exit)), _) => Check::Exited(exit),
                    (_, Ok(Some(runtime))) => Check::Running(runtime),
                    _ => Check::Absent,
                };
                (id, check)
            })
            .collect();
        let now = now_ms();
        let mut connect = Vec::new();
        {
            let mut inner = self.inner();
            inner.sessions = sessions;
            inner.ui = ui;
            let ids: HashSet<AgentId> = records.iter().map(|record| record.id).collect();
            inner.agents.retain(|id, _| ids.contains(id));
            for record in records {
                let id = record.id;
                let fresh = now.saturating_sub(record.created_at_ms) < FRESH_RECORD_MS;
                let tracked = inner.agents.entry(id).or_insert_with(|| Tracked {
                    record: record.clone(),
                    link: Link::Pending,
                    status: None,
                    generation: 0,
                });
                tracked.record = record;
                match checks.get(&id) {
                    None => {}
                    Some(Check::Exited(exit)) => tracked.link = Link::Exited(exit.clone()),
                    Some(Check::Running(runtime)) if !matches!(tracked.link, Link::Lost(Some(failed)) if failed == runtime.started_at_ms) =>
                    {
                        tracked.generation += 1;
                        tracked.link = Link::Connecting;
                        connect.push((id, tracked.generation, runtime.clone()));
                    }
                    Some(Check::Running(_)) => {}
                    Some(Check::Absent) if fresh => tracked.link = Link::Pending,
                    Some(Check::Absent) => {
                        tracked.status = None;
                        if !matches!(tracked.link, Link::Lost(_)) {
                            tracked.link = Link::Lost(None);
                        }
                    }
                }
            }
        }
        for (id, generation, runtime) in connect {
            let hub = Arc::clone(self);
            let _ =
                thread::Builder::new().name("orkai-pwa-link".into()).spawn(move || hub.link(id, generation, &runtime));
        }
    }

    fn link(self: Arc<Self>, id: AgentId, generation: u64, runtime: &RuntimeInfo) {
        let result = client::connect(runtime);
        let mut inner = self.inner();
        let Some(tracked) = inner.agents.get_mut(&id).filter(|tracked| tracked.generation == generation) else {
            return;
        };
        match result {
            Ok((conn, reader, _)) => {
                tracked.link = Link::Live(Arc::new(conn));
                drop(inner);
                let hub = Arc::clone(&self);
                let _ = client::spawn_reader(reader, move |message| hub.on_holder(id, generation, message));
            }
            Err(_) => {
                tracked.link = Link::Lost(Some(runtime.started_at_ms));
                inner.due = true;
                drop(inner);
                self.wake.notify_all();
            }
        }
    }

    fn on_holder(&self, id: AgentId, generation: u64, message: Option<HolderMsg>) -> bool {
        let mut inner = self.inner();
        let Some(tracked) = inner.agents.get_mut(&id).filter(|tracked| tracked.generation == generation) else {
            return false;
        };
        let keep = match message {
            Some(HolderMsg::Status(status)) => {
                let changed = tracked.status.as_ref().is_none_or(|old| old.state != status.state);
                tracked.status = Some(status);
                if changed {
                    inner.due = true;
                }
                true
            }
            Some(HolderMsg::Exited(info)) => {
                tracked.link = Link::Exited(info);
                inner.due = true;
                true
            }
            Some(_) => true,
            None => {
                if !matches!(tracked.link, Link::Exited(_)) {
                    tracked.link = Link::Pending;
                }
                inner.due = true;
                false
            }
        };
        let notify = inner.due;
        drop(inner);
        if notify {
            self.wake.notify_all();
        }
        keep
    }

    fn overview(&self, inner: &Inner) -> Overview {
        let mut tracked: Vec<&Tracked> = inner.agents.values().collect();
        tracked.sort_by_key(|t| Reverse(t.record.last_used_ms(t.status.as_ref())));
        let agents: Vec<AgentView> = tracked.into_iter().map(Tracked::view).collect();
        let sessions = inner
            .sessions
            .iter()
            .map(|session| SessionView {
                name: session.name.clone(),
                agents: agents.iter().filter(|a| a.session == session.name && a.state != AgentState::Exited).count(),
            })
            .collect();
        let home = crate::paths::user_home();
        let shown = |path: &std::path::Path| display_path(path, home.as_deref());
        let ui = &inner.ui;
        Overview {
            host: self.host.clone(),
            now: 0,
            session: ui.session.clone(),
            sessions,
            agents,
            where_dir: ui
                .session_dirs
                .get(&ui.session)
                .or(ui.where_dir.as_ref())
                .map(|dir| shown(dir))
                .unwrap_or_default(),
            dirs: ui.session_dirs.iter().map(|(session, dir)| (session.clone(), shown(dir))).collect(),
            worktree: ui.worktree,
            kinds: self.kinds.clone(),
        }
    }

    fn publish(&self) {
        let overview = {
            let mut inner = self.inner();
            let overview = self.overview(&inner);
            if inner.published.as_ref() == Some(&overview) {
                return;
            }
            inner.published = Some(overview.clone());
            overview
        };
        self.broadcast(&stamped(overview));
    }

    fn refresh_usage(self: &Arc<Self>) {
        {
            let mut inner = self.inner();
            if inner.usage_busy || inner.usage_at.is_some_and(|at| at.elapsed() < usage::REFRESH_EVERY) {
                return;
            }
            inner.usage_busy = true;
        }
        let hub = Arc::clone(self);
        let _ = thread::Builder::new().name("orkai-pwa-usage".into()).spawn(move || {
            let config = hub.launcher.config();
            let entries: Vec<UsageEntry> = thread::scope(|scope| {
                let fetches: Vec<_> = usage::PROVIDERS
                    .iter()
                    .map(|kind| scope.spawn(move || usage_entry(*kind, usage::fetch(*kind, config))))
                    .collect();
                fetches.into_iter().filter_map(|fetch| fetch.join().ok().flatten()).collect()
            });
            {
                let mut inner = hub.inner();
                inner.usage = Some(entries.clone());
                inner.usage_at = Some(Instant::now());
                inner.usage_busy = false;
            }
            hub.broadcast(&BridgeMsg::Usage { entries });
        });
    }
}

fn stamped(overview: Overview) -> BridgeMsg {
    BridgeMsg::State(Overview { now: now_ms(), ..overview })
}

pub fn encode(message: &BridgeMsg) -> Vec<u8> {
    serde_json::to_vec(message).unwrap_or_default()
}

pub fn usage_entry(kind: AgentKind, result: Result<Limits, Problem>) -> Option<UsageEntry> {
    let limits = match result {
        Err(Problem::SignedOut) => return None,
        Err(problem) => {
            return Some(UsageEntry {
                kind,
                windows: Vec::new(),
                credits: None,
                problem: Some(problem.label().to_string()),
            });
        }
        Ok(limits) => limits,
    };
    let windows = [
        (USAGE_SESSION_LABEL, limits.session),
        (USAGE_WEEKLY_LABEL, limits.weekly),
        (USAGE_MONTHLY_LABEL, limits.monthly),
    ]
    .into_iter()
    .filter_map(|(label, window)| {
        window.map(|w| UsageWindow { label: label.to_string(), used: w.used_percent, resets_at: w.resets_at_ms })
    })
    .collect();
    Some(UsageEntry { kind, windows, credits: limits.credits, problem: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::Window;

    #[test]
    fn colors_become_css_hex() {
        assert_eq!(hex(Color::Rgb(255, 95, 135)), "#ff5f87");
        assert_eq!(hex(Color::Reset), "#9696a0");
    }

    #[test]
    fn a_signed_out_provider_is_left_out_of_the_usage() {
        assert_eq!(usage_entry(AgentKind::Claude, Err(Problem::SignedOut)), None);
        let expired = usage_entry(AgentKind::Codex, Err(Problem::Expired)).unwrap();
        assert_eq!(expired.problem.as_deref(), Some(Problem::Expired.label()));
        assert!(expired.windows.is_empty());
    }

    #[test]
    fn the_usage_keeps_the_windows_the_plan_has_in_the_overview_order() {
        let limits = Limits {
            session: Some(Window { used_percent: 42.0, resets_at_ms: Some(9) }),
            weekly: None,
            monthly: Some(Window { used_percent: 7.5, resets_at_ms: None }),
            credits: Some(3.25),
        };
        let entry = usage_entry(AgentKind::Claude, Ok(limits)).unwrap();
        let labels: Vec<&str> = entry.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, [USAGE_SESSION_LABEL, USAGE_MONTHLY_LABEL]);
        assert_eq!(entry.windows[0].resets_at, Some(9));
        assert_eq!(entry.credits, Some(3.25));
    }
}
