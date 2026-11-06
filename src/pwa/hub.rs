use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::client::{self, Connection};
use crate::config::Config;
use crate::launch::{self, Launcher};
use crate::model::{
    AgentId, AgentKind, AgentRecord, AgentState, ExitInfo, RuntimeInfo, Session, StatusReport, UiState, Worktree,
};
use crate::paths::Home;
use crate::protocol::{ClientMsg, HolderMsg};
use crate::pwa::protocol::{
    AgentView, BridgeMsg, ChangeCount, Overview, PhoneMsg, SessionView, SubagentView, UsageEntry, UsageWindow,
};
use crate::store::Store;
use crate::subagents::{self, Subagent, SubagentWatcher};
use crate::tui::format::{display_path, project_label};
use crate::tui::render::{USAGE_MONTHLY_LABEL, USAGE_SESSION_LABEL, USAGE_WEEKLY_LABEL};
use crate::tui::theme;
use crate::usage::{self, Limits, Problem};
use crate::util::{lock, now_ms};
use crate::worktree::{ChangeWatcher, Changes};

pub const WATCH_EVERY: Duration = Duration::from_millis(500);
const RESCAN_EVERY: Duration = Duration::from_secs(1);
const SCAN_EVERY: Duration = Duration::from_secs(2);
const FRESH_RECORD_MS: u64 = 60_000;
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
    changes: Option<Changes>,
    subagents: Vec<Subagent>,
    model: Option<String>,
    launch_model: Option<String>,
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

    fn launching(&self) -> bool {
        self.record.launches <= 1 && now_ms().saturating_sub(self.record.created_at_ms) < FRESH_RECORD_MS
    }

    fn starting(&self) -> bool {
        self.launching() && self.status.is_none() && !self.is_dead()
    }

    fn attaching(&self) -> bool {
        matches!(self.link, Link::Connecting | Link::Live(_)) && self.status.is_none()
    }

    fn view(&self) -> AgentView {
        let live = self.status.as_ref().filter(|_| !self.is_dead());
        let starting = self.starting();
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
            changes: self.record.worktree.as_ref().and(self.changes.as_ref()).map(|changes| ChangeCount {
                files: changes.files,
                insertions: changes.insertions,
                deletions: changes.deletions,
            }),
            test: live.and_then(|status| status.test.as_ref()).map(|test| test.state),
            subagents: self
                .subagents
                .iter()
                .map(|sub| SubagentView {
                    role: sub.role.clone(),
                    description: sub.description.clone(),
                    activity: sub.activity.clone(),
                    started_at: sub.started_at_ms,
                })
                .collect(),
            model: live.and_then(|status| self.running_model(status)),
        }
    }

    fn running_model(&self, status: &StatusReport) -> Option<String> {
        let launched = self.launch_model.clone().filter(|_| status.nested.is_none());
        self.model.clone().or(launched)
    }

    fn parent(&self) -> Option<subagents::Parent> {
        let live = self.status.as_ref().filter(|_| !self.is_dead())?;
        let kind = live.running_kind(self.record.kind);
        subagents::parent_of(&self.record, kind, live.nested.as_ref(), self.state())
    }
}

struct Scanners {
    changes: ChangeWatcher,
    subagents: SubagentWatcher,
}

pub fn hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#9696a0".to_string(),
    }
}

const MAX_DEVICE_TEXT: usize = 300;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Device {
    pub id: u64,
    pub agent: String,
    pub since_ms: u64,
    pub model: Option<String>,
    pub os_version: Option<String>,
    pub standalone: bool,
    #[serde(skip)]
    pub client: Option<String>,
}

pub fn device_text(text: &str) -> Option<String> {
    let clean: String = text.chars().filter(|c| !c.is_control()).take(MAX_DEVICE_TEXT).collect();
    let clean = clean.trim();
    (!clean.is_empty()).then(|| clean.to_string())
}

#[derive(Default)]
struct Inner {
    phones: BTreeMap<u64, Sender<Event>>,
    devices: BTreeMap<u64, Device>,
    next_phone: u64,
    agents: BTreeMap<AgentId, Tracked>,
    sessions: Vec<Session>,
    ui: UiState,
    published: Option<Overview>,
    usage: Option<Vec<UsageEntry>>,
    usage_at: Option<Instant>,
    usage_busy: bool,
    scan_at: Option<Instant>,
    scan_busy: bool,
    relaunching: HashSet<AgentId>,
    watching: bool,
    due: bool,
}

pub struct Relaunch {
    hub: Arc<Hub>,
    id: AgentId,
}

impl Drop for Relaunch {
    fn drop(&mut self) {
        self.hub.inner().relaunching.remove(&self.id);
        self.hub.nudge();
    }
}

pub struct Hub {
    store: Store,
    scanners: Mutex<Scanners>,
    tools: Mutex<Tools>,
    host: String,
    inner: Mutex<Inner>,
    wake: Condvar,
}

struct Tools {
    launcher: Arc<Launcher>,
    kinds: Vec<AgentKind>,
}

impl Tools {
    fn new(launcher: Launcher) -> Self {
        let kinds = launch::installed_kinds(launcher.config());
        Self { launcher: Arc::new(launcher), kinds }
    }
}

enum Check {
    Exited(ExitInfo),
    Running(RuntimeInfo),
    Absent,
}

impl Hub {
    pub fn new(launcher: Launcher, host: String) -> Arc<Self> {
        Arc::new(Self {
            store: launcher.store().clone(),
            scanners: Mutex::new(Scanners {
                changes: ChangeWatcher::default(),
                subagents: SubagentWatcher::new(launch::claude_home(), launch::codex_home()),
            }),
            tools: Mutex::new(Tools::new(launcher)),
            host,
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

    pub fn launcher(&self) -> Arc<Launcher> {
        Arc::clone(&lock(&self.tools).launcher)
    }

    pub fn reconfigure(&self, config: Config) -> io::Result<()> {
        let launcher = self.launcher().reconfigured(config)?;
        *lock(&self.tools) = Tools::new(launcher);
        self.nudge();
        Ok(())
    }

    pub fn recheck_kinds(&self) {
        let launcher = self.launcher();
        let kinds = launch::installed_kinds(launcher.config());
        {
            let mut tools = lock(&self.tools);
            if Arc::ptr_eq(&tools.launcher, &launcher) {
                tools.kinds = kinds;
            }
        }
        self.nudge();
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn kinds(&self) -> Vec<AgentKind> {
        lock(&self.tools).kinds.clone()
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
        let mut inner = self.inner();
        inner.phones.remove(&id);
        inner.devices.remove(&id);
        drop(inner);
        self.wake.notify_all();
    }

    pub fn describe(&self, id: u64, device: Device) {
        let mut inner = self.inner();
        if inner.phones.contains_key(&id) {
            inner.devices.insert(id, Device { id, ..device });
        }
    }

    pub fn refine(
        &self,
        id: u64,
        model: Option<String>,
        os_version: Option<String>,
        standalone: bool,
        client: Option<String>,
    ) {
        let mut inner = self.inner();
        let client = client.as_deref().and_then(device_text);
        if client.is_some() {
            let stale: Vec<u64> = inner
                .devices
                .values()
                .filter(|device| device.id != id && device.client == client)
                .map(|device| device.id)
                .collect();
            for old in stale {
                inner.phones.remove(&old);
                inner.devices.remove(&old);
            }
        }
        if let Some(device) = inner.devices.get_mut(&id) {
            device.model = model.as_deref().and_then(device_text);
            device.os_version = os_version.as_deref().and_then(device_text);
            device.standalone = standalone;
            device.client = client;
        }
        drop(inner);
        self.wake.notify_all();
    }

    pub fn devices(&self) -> Vec<Device> {
        let inner = self.inner();
        inner.devices.values().filter(|device| inner.phones.contains_key(&device.id)).cloned().collect()
    }

    pub fn phones(&self) -> usize {
        self.inner().phones.len()
    }

    pub(crate) fn has_phone(&self, id: u64) -> bool {
        self.inner().phones.contains_key(&id)
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

    pub fn try_relaunch(self: &Arc<Self>, id: AgentId) -> Option<Relaunch> {
        self.inner().relaunching.insert(id).then(|| Relaunch { hub: Arc::clone(self), id })
    }

    pub fn race_members(&self, id: AgentId) -> Vec<AgentRecord> {
        let inner = self.inner();
        let Some(record) = inner.agents.get(&id).map(|tracked| &tracked.record) else { return Vec::new() };
        let Some(race) = record.race else { return Vec::new() };
        inner
            .agents
            .values()
            .map(|tracked| &tracked.record)
            .filter(|other| other.race == Some(race) && other.worktree.is_some())
            .cloned()
            .collect()
    }

    pub fn live_status(&self, id: AgentId) -> Option<StatusReport> {
        self.inner().agents.get(&id).filter(|tracked| !tracked.is_dead()).and_then(|tracked| tracked.status.clone())
    }

    pub fn worktrees(&self) -> Vec<(AgentId, String, Worktree)> {
        self.inner()
            .agents
            .values()
            .filter_map(|tracked| {
                tracked
                    .record
                    .worktree
                    .clone()
                    .map(|worktree| (tracked.record.id, tracked.record.name.clone(), worktree))
            })
            .collect()
    }

    pub fn lost(&self) -> Vec<AgentRecord> {
        self.inner()
            .agents
            .values()
            .filter(|tracked| matches!(tracked.link, Link::Lost(_)))
            .map(|tracked| tracked.record.clone())
            .collect()
    }

    pub fn lost_in(&self, session: &str) -> Vec<AgentRecord> {
        self.lost().into_iter().filter(|record| record.session == session).collect()
    }

    pub fn settled(&self) -> bool {
        let inner = self.inner();
        inner.published.is_some()
            && inner.agents.values().all(|tracked| !matches!(tracked.link, Link::Pending | Link::Connecting))
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
                client::deliver(&runtime, message)
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
            self.refresh_scan();
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
                    changes: None,
                    subagents: Vec::new(),
                    model: None,
                    launch_model: None,
                });
                tracked.record = record;
                match checks.get(&id) {
                    None => {}
                    Some(Check::Exited(exit)) => tracked.link = Link::Exited(exit.clone()),
                    Some(Check::Running(runtime)) if !matches!(tracked.link, Link::Lost(Some(failed)) if failed == runtime.started_at_ms) =>
                    {
                        if tracked.status.as_ref().is_some_and(|old| old.started_at_ms != runtime.started_at_ms) {
                            tracked.status = None;
                        }
                        tracked.generation += 1;
                        tracked.link = Link::Connecting;
                        connect.push((id, tracked.generation, runtime.clone()));
                    }
                    Some(Check::Running(_)) => {}
                    Some(Check::Absent) => {
                        tracked.status = None;
                        if fresh {
                            tracked.link = Link::Pending;
                        } else if !matches!(tracked.link, Link::Lost(_)) {
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
        let launch_model = self.store.read_launch(id).ok().flatten().and_then(|spec| launch::launch_model(&spec));
        let mut inner = self.inner();
        let Some(tracked) = inner.agents.get_mut(&id).filter(|tracked| tracked.generation == generation) else {
            return;
        };
        match result {
            Ok((conn, reader, _)) => {
                tracked.link = Link::Live(Arc::new(conn));
                tracked.launch_model = launch_model;
                drop(inner);
                let hub = Arc::clone(&self);
                let _ = client::spawn_reader(reader, move |message| hub.on_holder(id, generation, message));
            }
            Err(err) => {
                let slow = matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock);
                tracked.link =
                    if slow && tracked.launching() { Link::Pending } else { Link::Lost(Some(runtime.started_at_ms)) };
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
        let session = if inner.sessions.iter().any(|session| session.name == ui.session) {
            ui.session.clone()
        } else {
            inner.sessions.first().map_or_else(|| ui.session.clone(), |session| session.name.clone())
        };
        Overview {
            host: self.host.clone(),
            now: 0,
            where_dir: ui
                .session_dirs
                .get(&session)
                .or(ui.where_dir.as_ref())
                .map(|dir| shown(dir))
                .unwrap_or_default(),
            session,
            sessions,
            agents,
            dirs: ui.session_dirs.iter().map(|(session, dir)| (session.clone(), shown(dir))).collect(),
            worktree: ui.worktree,
            kinds: self.kinds(),
        }
    }

    fn publish(&self) {
        let overview = {
            let mut inner = self.inner();
            if inner.published.is_none() && inner.agents.values().any(Tracked::attaching) {
                return;
            }
            let overview = self.overview(&inner);
            if inner.published.as_ref() == Some(&overview) {
                return;
            }
            inner.published = Some(overview.clone());
            overview
        };
        self.broadcast(&stamped(overview));
    }

    fn refresh_scan(self: &Arc<Self>) {
        let (targets, parents) = {
            let mut inner = self.inner();
            if inner.scan_busy || inner.scan_at.is_some_and(|at| at.elapsed() < SCAN_EVERY) {
                return;
            }
            inner.scan_busy = true;
            let targets: Vec<(AgentId, Worktree)> = inner
                .agents
                .values()
                .filter_map(|tracked| tracked.record.worktree.clone().map(|worktree| (tracked.record.id, worktree)))
                .collect();
            let parents: Vec<subagents::Parent> = inner.agents.values().filter_map(Tracked::parent).collect();
            (targets, parents)
        };
        let hub = Arc::clone(self);
        let spawned = thread::Builder::new().name("orkai-pwa-scan".into()).spawn(move || {
            let (changes, mut found, mut models) = {
                let mut scanners = lock(&hub.scanners);
                let now = now_ms();
                let changes = scanners.changes.scan(&targets, now);
                let found = scanners.subagents.scan(&parents, now);
                let models = scanners.subagents.models(&parents);
                (changes, found, models)
            };
            let mut inner = hub.inner();
            for tracked in inner.agents.values_mut() {
                let id = tracked.record.id;
                if let Some(changed) = changes.get(&id) {
                    tracked.changes = Some(changed.clone());
                }
                tracked.subagents = found.remove(&id).unwrap_or_default();
                tracked.model = models.remove(&id);
            }
            inner.scan_busy = false;
            inner.scan_at = Some(Instant::now());
            inner.due = true;
            drop(inner);
            hub.wake.notify_all();
        });
        if spawned.is_err() {
            self.inner().scan_busy = false;
        }
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
            let launcher = hub.launcher();
            let config = launcher.config();
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
    fn after_a_reboot_the_hub_settles_with_the_interrupted_agents_and_not_the_finished_ones() {
        use crate::model::{ExitInfo, NewAgent, RuntimeInfo};
        use crate::protocol::PROTOCOL_VERSION;

        let dir = std::env::temp_dir().join(format!("orkai-hub-settle-{}-{}", std::process::id(), now_ms()));
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        let agent = |name: &str, session: &str| {
            let draft = NewAgent {
                session: session.into(),
                name: name.into(),
                kind: AgentKind::Terminal,
                cwd: dir.clone(),
                task: String::new(),
            };
            store.create_session(session).ok();
            let mut record = store.create_agent(draft).unwrap();
            record.created_at_ms = 1;
            store.save_agent(&record).unwrap();
            record
        };
        let interrupted = agent("interrupted", "default");
        let finished = agent("finished", "other");
        let dead_port =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        let runtime = RuntimeInfo {
            holder_pid: 1,
            child_pid: None,
            port: dead_port,
            token: "gone".into(),
            started_at_ms: now_ms(),
            protocol: PROTOCOL_VERSION,
            version: "test".into(),
            exe: None,
        };
        crate::fsutil::write_json_atomic(&store.home().runtime_file(interrupted.id), &runtime).unwrap();
        let exit = ExitInfo { code: Some(0), error: None, at_ms: now_ms() };
        crate::fsutil::write_json_atomic(&store.home().exit_file(finished.id), &exit).unwrap();

        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        let hub = Hub::new(launcher, "host".into());
        assert!(!hub.settled(), "nothing is settled before the first scan");
        let (tx, _rx) = std::sync::mpsc::channel();
        hub.join(tx);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !hub.settled() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(hub.settled());
        let lost: Vec<AgentId> = hub.lost().iter().map(|record| record.id).collect();
        assert_eq!(lost, [interrupted.id]);
        assert_eq!(hub.lost_in("default").len(), 1);
        assert!(hub.lost_in("other").is_empty(), "a finished agent is not recovered");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tracked_with_status(nested: Option<crate::model::NestedAgent>) -> Tracked {
        let record = AgentRecord {
            id: 1,
            session: "default".into(),
            name: "a".into(),
            kind: AgentKind::Claude,
            cwd: "/work".into(),
            task: String::new(),
            created_at_ms: 0,
            conversation_id: None,
            launches: 1,
            worktree: None,
            bypass: false,
            remote_control: false,
            project: None,
            parent: None,
            race: None,
        };
        let status = StatusReport {
            state: AgentState::Working,
            state_since_ms: 0,
            turn_started_ms: None,
            last_turn_ms: None,
            started_at_ms: 0,
            title: String::new(),
            activity: String::new(),
            rows: 24,
            cols: 80,
            exit: None,
            nested,
            viewers: None,
            test: None,
            input_at_ms: 0,
        };
        Tracked {
            record,
            link: Link::Pending,
            status: Some(status),
            generation: 0,
            changes: None,
            subagents: Vec::new(),
            model: None,
            launch_model: None,
        }
    }

    #[test]
    fn an_agent_shows_the_model_its_session_reports_before_the_one_it_was_launched_with() {
        let mut tracked = tracked_with_status(None);
        assert_eq!(tracked.view().model, None, "nothing known means nothing shown");
        tracked.launch_model = Some("opus".into());
        assert_eq!(tracked.view().model.as_deref(), Some("opus"));
        tracked.model = Some("claude-sonnet-4-5".into());
        assert_eq!(tracked.view().model.as_deref(), Some("claude-sonnet-4-5"), "/model wins over the launch");
        tracked.link = Link::Lost(None);
        assert_eq!(tracked.view().model, None, "a dead agent runs no model");
    }

    #[test]
    fn a_tool_started_by_hand_in_a_terminal_never_takes_the_launch_model() {
        let nested = crate::model::NestedAgent {
            kind: AgentKind::Claude,
            pid: 2,
            conversation_id: None,
            cwd: None,
            since_ms: 0,
        };
        let mut tracked = tracked_with_status(Some(nested));
        tracked.launch_model = Some("opus".into());
        assert_eq!(tracked.view().model, None);
        tracked.model = Some("claude-opus-5-5".into());
        assert_eq!(tracked.view().model.as_deref(), Some("claude-opus-5-5"));
    }

    fn tracked_agent(record: AgentRecord, link: Link, status: Option<StatusReport>) -> Tracked {
        Tracked {
            record,
            link,
            status,
            generation: 1,
            changes: None,
            subagents: Vec::new(),
            model: None,
            launch_model: None,
        }
    }

    fn record_of(dir: &std::path::Path, launches: u32, created_at_ms: u64) -> (Store, AgentRecord) {
        use crate::model::NewAgent;
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        let draft = NewAgent {
            session: "default".into(),
            name: "agent".into(),
            kind: AgentKind::Claude,
            cwd: dir.to_path_buf(),
            task: "fix the login".into(),
        };
        let mut record = store.create_agent(draft).unwrap();
        record.launches = launches;
        record.created_at_ms = created_at_ms;
        store.save_agent(&record).unwrap();
        (store, record)
    }

    fn report_of_a_run(started_at: u64, state: AgentState) -> StatusReport {
        use crate::holder::term::TermState;
        use crate::protocol::TermSize;
        let term = TermState::new(AgentKind::Claude, TermSize::new(24, 80), started_at, String::new(), None);
        StatusReport { state, ..term.report(None) }
    }

    fn live_link() -> Link {
        use crate::model::RuntimeInfo;
        use crate::protocol::{PROTOCOL_VERSION, Welcome};
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            ClientMsg::read(&mut stream).unwrap();
            let welcome = Welcome { agent_id: 1, protocol: PROTOCOL_VERSION, version: "test".into(), holder_pid: 1 };
            std::io::Write::write_all(&mut stream, &HolderMsg::Welcome(welcome).to_frame()).unwrap();
            let _ = ClientMsg::read(&mut stream);
        });
        let runtime = RuntimeInfo {
            holder_pid: 1,
            child_pid: None,
            port,
            token: "test".into(),
            started_at_ms: 1,
            protocol: PROTOCOL_VERSION,
            version: "test".into(),
            exe: None,
        };
        let (conn, _, _) = client::connect(&runtime).unwrap();
        Link::Live(Arc::new(conn))
    }

    #[test]
    fn a_launch_is_starting_until_the_first_status_arrives_even_once_it_is_linked() {
        let dir = std::env::temp_dir().join(format!("orkai-hub-starting-{}-{}", std::process::id(), now_ms()));
        let (_store, record) = record_of(&dir, 1, now_ms());
        for link in [Link::Pending, Link::Connecting, live_link()] {
            let view = tracked_agent(record.clone(), link, None).view();
            assert!(view.starting, "nothing has reported yet");
            assert_eq!(view.activity, STARTING);
        }
        let reported = report_of_a_run(now_ms(), AgentState::Working);
        let view = tracked_agent(record.clone(), Link::Connecting, Some(reported)).view();
        assert!(!view.starting);
        assert_eq!(view.state, AgentState::Working);
        let info = ExitInfo { code: Some(3), error: None, at_ms: now_ms() };
        assert!(!tracked_agent(record.clone(), Link::Exited(info), None).view().starting, "a crash ends it");
        assert!(!tracked_agent(record, Link::Lost(None), None).view().starting);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovered_restarted_and_old_agents_are_not_shown_as_starting() {
        let dir = std::env::temp_dir().join(format!("orkai-hub-not-starting-{}-{}", std::process::id(), now_ms()));
        let (_store, restarted) = record_of(&dir, 2, now_ms());
        let (_store, old) = record_of(&dir, 1, now_ms().saturating_sub(FRESH_RECORD_MS));
        for record in [restarted, old] {
            for link in [Link::Pending, Link::Connecting, live_link()] {
                let view = tracked_agent(record.clone(), link, None).view();
                assert!(!view.starting, "launch {} of a record from {}", record.launches, record.created_at_ms);
                assert_ne!(view.state, AgentState::Working);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_run_forgets_the_status_of_the_last_one_and_a_reconnection_keeps_its_own() {
        use crate::model::RuntimeInfo;
        use crate::protocol::PROTOCOL_VERSION;

        let dir = std::env::temp_dir().join(format!("orkai-hub-new-run-{}-{}", std::process::id(), now_ms()));
        let (store, record) = record_of(&dir, 2, now_ms());
        let dead_port =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        let runtime = RuntimeInfo {
            holder_pid: 1,
            child_pid: None,
            port: dead_port,
            token: "gone".into(),
            started_at_ms: 500,
            protocol: PROTOCOL_VERSION,
            version: "test".into(),
            exe: None,
        };
        crate::fsutil::write_json_atomic(&store.home().runtime_file(record.id), &runtime).unwrap();
        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        let hub = Hub::new(launcher, "host".into());
        let run_status = |started: u64| {
            let report = report_of_a_run(started, AgentState::Working);
            let tracked = tracked_agent(record.clone(), Link::Pending, Some(report));
            hub.inner().agents.insert(record.id, tracked);
            hub.rescan();
            hub.inner().agents[&record.id].status.as_ref().map(|status| status.started_at_ms)
        };
        assert_eq!(run_status(400), None, "the run that was stopped while working says nothing about this one");
        assert_eq!(run_status(500), Some(500), "a dropped link to the same run keeps what it reported");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_first_overview_waits_for_the_agents_being_linked_to_report() {
        let dir = std::env::temp_dir().join(format!("orkai-hub-first-{}-{}", std::process::id(), now_ms()));
        let (store, record) = record_of(&dir, 1, 1);
        let id = record.id;
        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        let hub = Hub::new(launcher, "host".into());
        hub.inner().agents.insert(id, tracked_agent(record, Link::Connecting, None));
        hub.publish();
        assert!(hub.inner().published.is_none(), "the board does not open on a state nobody reported");
        hub.on_holder(id, 1, Some(HolderMsg::Status(report_of_a_run(1, AgentState::Working))));
        hub.publish();
        let published = hub.inner().published.clone().expect("every linked agent has reported");
        assert_eq!(published.agents[0].state, AgentState::Working);
        hub.inner().agents.get_mut(&id).unwrap().status = None;
        hub.publish();
        assert!(hub.inner().published.is_some(), "only the first overview waits");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_holder_too_busy_to_answer_a_launch_is_asked_again_and_only_an_old_agent_is_lost() {
        use crate::model::RuntimeInfo;
        use crate::protocol::PROTOCOL_VERSION;

        let dir = std::env::temp_dir().join(format!("orkai-hub-slow-{}-{}", std::process::id(), now_ms()));
        let (store, record) = record_of(&dir, 1, now_ms());
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let runtime = RuntimeInfo {
            holder_pid: 1,
            child_pid: None,
            port: listener.local_addr().unwrap().port(),
            token: "test".into(),
            started_at_ms: 500,
            protocol: PROTOCOL_VERSION,
            version: "test".into(),
            exe: None,
        };
        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        let hub = Hub::new(launcher, "host".into());
        let after_a_silent_holder = |record: AgentRecord| {
            let id = record.id;
            hub.inner().agents.insert(id, tracked_agent(record, Link::Connecting, None));
            Arc::clone(&hub).link(id, 1, &runtime);
            matches!(hub.inner().agents[&id].link, Link::Pending)
        };
        assert!(after_a_silent_holder(record.clone()), "a launch stays starting and the next scan links it");
        let old = AgentRecord { created_at_ms: 1, ..record };
        assert!(!after_a_silent_holder(old), "an old agent that does not answer is lost");
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn colors_become_css_hex() {
        assert_eq!(hex(Color::Rgb(255, 95, 135)), "#ff5f87");
        assert_eq!(hex(Color::Reset), "#9696a0");
    }

    #[test]
    fn a_racer_moved_to_another_session_stays_in_its_race() {
        use crate::model::NewAgent;

        let dir = std::env::temp_dir().join(format!("orkai-hub-race-{}-{}", std::process::id(), now_ms()));
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        store.create_session("work").unwrap();
        let racer = |name: &str| {
            let draft = NewAgent {
                session: "default".into(),
                name: name.into(),
                kind: AgentKind::Claude,
                cwd: dir.clone(),
                task: String::new(),
            };
            let mut record = store.create_agent(draft).unwrap();
            record.race = Some(7);
            record.worktree = Some(Worktree {
                repo: dir.clone(),
                path: dir.join(name),
                branch: name.into(),
                base: "HEAD".into(),
                source: dir.clone(),
            });
            store.save_agent(&record).unwrap();
            record
        };
        let (first, second) = (racer("first"), racer("second"));
        let launcher = Launcher::new(store.clone(), Config::default(), &std::env::current_exe().unwrap()).unwrap();
        let hub = Hub::new(launcher, "host".into());
        store.move_agent(second.id, "work").unwrap();
        hub.rescan();
        let ids = |of| hub.race_members(of).into_iter().map(|record| record.id).collect::<Vec<_>>();
        assert_eq!(ids(first.id), [first.id, second.id]);
        assert_eq!(ids(second.id), [first.id, second.id]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn connected_devices_are_bounded_and_reconnecting_tabs_replace_old_entries() {
        let dir = std::env::temp_dir().join(format!("orkai-hub-devices-{}-{}", std::process::id(), now_ms()));
        let store = Store::open(Home::at(dir.join("home"))).unwrap();
        let launcher = Launcher::new(store, Config::default(), &std::env::current_exe().unwrap()).unwrap();
        let hub = Hub::new(launcher, "host".into());
        let (tx, _rx) = std::sync::mpsc::channel();
        let id = hub.join(tx);
        hub.describe(id, Device { agent: "Safari".into(), since_ms: 5, ..Device::default() });
        hub.refine(id, Some(format!(" {}\u{7} ", "A".repeat(400))), None, true, Some("tab-1".into()));
        assert!(hub.devices()[0].model.as_ref().unwrap().len() <= 300);
        assert!(!hub.devices()[0].model.as_ref().unwrap().contains('\u{7}'));
        let (next_tx, _next_rx) = std::sync::mpsc::channel();
        let next = hub.join(next_tx);
        hub.describe(next, Device { agent: "Chrome".into(), ..Device::default() });
        hub.refine(next, None, None, false, Some("tab-1".into()));
        assert_eq!(hub.phones(), 1);
        assert!(!hub.has_phone(id));
        assert_eq!(hub.devices().iter().map(|device| device.id).collect::<Vec<_>>(), vec![next]);
        hub.leave(id);
        assert_eq!(hub.phones(), 1);
        hub.leave(next);
        assert!(hub.devices().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
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
