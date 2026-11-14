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
use crate::orchestrator::relay::Follower;
use crate::orchestrator::runner::Jobs;
use crate::orchestrator::{CliEngine, Engine, Orchestrator, OrchestratorView, Sight, Sighting};
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
use crate::usage::{self, Usage};
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
    child_pid: Option<u32>,
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

    fn shown_state(&self) -> AgentState {
        match self.state() {
            AgentState::Done | AgentState::Idle if !self.subagents.is_empty() => AgentState::Working,
            state => state,
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
            state: self.shown_state(),
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

    fn sighting(&self) -> Sighting {
        let sight = match (&self.status, self.is_dead()) {
            (None, false) => Sight::Unknown,
            (Some(status), false) => Sight::of(status.state, status.state_since_ms),
            (_, true) => Sight::Other,
        };
        (self.record.id, sight)
    }

    fn parent(&self) -> Option<subagents::Parent> {
        let live = self.status.as_ref().filter(|_| !self.is_dead())?;
        let kind = live.running_kind(self.record.kind);
        subagents::parent_of(&self.record, kind, live.nested.as_ref(), self.state(), self.child_pid)
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
    limits: BTreeMap<AgentKind, Usage>,
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
    orchestrator: Option<Arc<Orchestrator>>,
    follower: Mutex<Follower>,
    jobs: Jobs,
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
        Self::build(launcher, host, None)
    }

    pub fn orchestrating(launcher: Launcher, host: String, engine: Arc<dyn Engine>) -> Arc<Self> {
        let orchestrator = Orchestrator::open(launcher.store().home(), engine);
        Self::build(launcher, host, Some(orchestrator))
    }

    fn build(launcher: Launcher, host: String, orchestrator: Option<Arc<Orchestrator>>) -> Arc<Self> {
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
            orchestrator,
            follower: Mutex::new(Follower::default()),
            jobs: Jobs::default(),
        })
    }

    fn launcher_at(home: &Home) -> io::Result<(Launcher, String)> {
        let store = Store::open(home.clone())?;
        let config = Config::load_or_init(home)?;
        let launcher = Launcher::new(store, config, &std::env::current_exe()?)?;
        let host = gethostname::gethostname().to_string_lossy().to_lowercase();
        Ok((launcher, host))
    }

    pub fn open(home: &Home) -> io::Result<Arc<Self>> {
        let (launcher, host) = Self::launcher_at(home)?;
        Ok(Self::new(launcher, host))
    }

    pub fn open_orchestrating(home: &Home) -> io::Result<Arc<Self>> {
        let (launcher, host) = Self::launcher_at(home)?;
        Ok(Self::orchestrating(launcher, host, Arc::new(CliEngine::new(home))))
    }

    pub fn orchestrator(&self) -> Option<&Arc<Orchestrator>> {
        self.orchestrator.as_ref()
    }

    pub fn jobs(&self) -> &Jobs {
        &self.jobs
    }

    pub fn followed(&self) -> OrchestratorView {
        let (changed, view) = {
            let mut follower = lock(&self.follower);
            (follower.refresh(self.store.home(), true), follower.shown())
        };
        if changed {
            self.broadcast(&BridgeMsg::Orchestrator(view.clone()));
        }
        view
    }

    pub fn agent_view(&self, id: AgentId) -> Option<AgentView> {
        self.inner().agents.get(&id).map(Tracked::view)
    }

    pub fn agent_views(&self) -> Vec<AgentView> {
        self.inner().agents.values().map(Tracked::view).collect()
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

    pub fn child_pid(&self, id: AgentId) -> Option<u32> {
        self.inner().agents.get(&id).filter(|tracked| !tracked.is_dead()).and_then(|tracked| tracked.child_pid)
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
            let rescan = due || rescanned.is_none_or(|at| at.elapsed() >= RESCAN_EVERY);
            if rescan {
                self.rescan();
                rescanned = Some(Instant::now());
            }
            self.publish();
            self.orchestrate(rescan);
            self.refresh_usage();
            self.refresh_scan();
            let inner = self.inner();
            let _ = self.wake.wait_timeout_while(inner, WATCH_EVERY, |inner| !inner.due && !inner.phones.is_empty());
        }
    }

    fn orchestrate(self: &Arc<Self>, rescanned: bool) {
        let Some(orchestrator) = self.orchestrator.as_ref() else { return self.follow(rescanned) };
        if !orchestrator.is_awake() {
            return;
        }
        orchestrator.relay(self, rescanned);
        if !orchestrator.is_enabled() {
            return;
        }
        let seen: Vec<Sighting> = self.inner().agents.values().map(Tracked::sighting).collect();
        orchestrator.observe(self, &seen);
    }

    fn follow(&self, rescanned: bool) {
        let changed = {
            let mut follower = lock(&self.follower);
            follower.refresh(self.store.home(), rescanned).then(|| follower.shown())
        };
        if let Some(view) = changed {
            self.broadcast(&BridgeMsg::Orchestrator(view));
        }
    }

    fn rescan(self: &Arc<Self>) {
        let Ok(records) = self.store.list_agents() else { return };
        let sessions = self.store.peek_sessions_with(&records).map(|(sessions, _)| sessions).unwrap_or_default();
        let ui = self.store.load_ui();
        if let Some(orchestrator) = self.orchestrator.as_ref() {
            orchestrator.set_relaying(ui.pwa);
        }
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
                    child_pid: None,
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
                tracked.child_pid = runtime.child_pid;
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
            let results: Vec<_> = thread::scope(|scope| {
                let fetches: Vec<_> = usage::PROVIDERS
                    .iter()
                    .map(|kind| (*kind, scope.spawn(move || usage::fetch(*kind, config))))
                    .collect();
                fetches.into_iter().filter_map(|(kind, fetch)| fetch.join().ok().map(|result| (kind, result))).collect()
            });
            let entries = {
                let mut inner = hub.inner();
                let now = now_ms();
                let entries: Vec<UsageEntry> = results
                    .into_iter()
                    .filter_map(|(kind, result)| {
                        let usage = inner.limits.entry(kind).or_default();
                        usage.apply(result, now);
                        usage_entry(kind, usage)
                    })
                    .collect();
                inner.usage = Some(entries.clone());
                inner.usage_at = Some(Instant::now());
                inner.usage_busy = false;
                entries
            };
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

pub fn usage_entry(kind: AgentKind, usage: &Usage) -> Option<UsageEntry> {
    if !usage.is_signed_in() {
        return None;
    }
    let Some(limits) = usage.limits else {
        return Some(UsageEntry {
            kind,
            windows: Vec::new(),
            credits: None,
            problem: usage.problem.map(|problem| problem.label().to_string()),
        });
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
    use crate::usage::{Limits, Problem, Window};

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
            model: None,
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
            child_pid: None,
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

    fn running_subagent() -> Subagent {
        Subagent {
            id: "a1".into(),
            role: "general-purpose".into(),
            description: "run the e2e suite".into(),
            activity: "Bash · cargo test".into(),
            started_at_ms: 0,
        }
    }

    #[test]
    fn an_agent_whose_subagents_still_run_is_shown_working_after_its_turn() {
        let mut tracked = tracked_with_status(None);
        for state in [AgentState::Done, AgentState::Idle] {
            tracked.status.as_mut().unwrap().state = state;
            tracked.subagents = vec![running_subagent()];
            assert_eq!(tracked.view().state, AgentState::Working, "{state:?} with a subagent running");
            assert_eq!(tracked.state(), state, "the holder's own state is kept for the subagent scan");
            tracked.subagents.clear();
            assert_eq!(tracked.view().state, state);
        }
        tracked.status.as_mut().unwrap().state = AgentState::Paused;
        tracked.subagents = vec![running_subagent()];
        assert_eq!(tracked.view().state, AgentState::Paused, "a question still waits for you");
        tracked.link = Link::Exited(ExitInfo { code: Some(0), error: None, at_ms: 0 });
        assert_eq!(tracked.view().state, AgentState::Exited);
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
            child_pid: None,
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

    fn fetched(results: &[Result<Limits, Problem>]) -> Usage {
        let mut usage = Usage::default();
        for (at, result) in results.iter().enumerate() {
            usage.apply(*result, u64::try_from(at).unwrap());
        }
        usage
    }

    #[test]
    fn a_signed_out_provider_is_left_out_of_the_usage() {
        assert_eq!(usage_entry(AgentKind::Claude, &fetched(&[Err(Problem::SignedOut)])), None);
        let expired = usage_entry(AgentKind::Codex, &fetched(&[Err(Problem::Expired)])).unwrap();
        assert_eq!(expired.problem.as_deref(), Some(Problem::Expired.label()));
        assert!(expired.windows.is_empty());
    }

    #[test]
    fn a_rate_limited_check_keeps_the_last_numbers_instead_of_showing_unavailable() {
        let limits = Limits { session: Some(Window { used_percent: 4.0, resets_at_ms: None }), ..Limits::default() };
        let kept = usage_entry(AgentKind::Claude, &fetched(&[Ok(limits), Err(Problem::Unavailable)])).unwrap();
        assert_eq!(kept.windows.iter().map(|w| w.used).collect::<Vec<_>>(), [4.0]);
        assert_eq!(kept.problem, None, "a failed refresh between two good ones is not news");

        let never = usage_entry(AgentKind::Claude, &fetched(&[Err(Problem::Unavailable)])).unwrap();
        assert!(never.windows.is_empty());
        assert_eq!(never.problem.as_deref(), Some(Problem::Unavailable.label()), "with no number yet it says so");

        let expired = usage_entry(AgentKind::Claude, &fetched(&[Ok(limits), Err(Problem::Expired)])).unwrap();
        assert!(expired.windows.is_empty(), "an expired login drops numbers it can no longer refresh");
        assert_eq!(expired.problem.as_deref(), Some(Problem::Expired.label()));
    }

    #[test]
    fn the_usage_keeps_the_windows_the_plan_has_in_the_overview_order() {
        let limits = Limits {
            session: Some(Window { used_percent: 42.0, resets_at_ms: Some(9) }),
            weekly: None,
            monthly: Some(Window { used_percent: 7.5, resets_at_ms: None }),
            credits: Some(3.25),
        };
        let entry = usage_entry(AgentKind::Claude, &fetched(&[Ok(limits)])).unwrap();
        let labels: Vec<&str> = entry.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, [USAGE_SESSION_LABEL, USAGE_MONTHLY_LABEL]);
        assert_eq!(entry.windows[0].resets_at, Some(9));
        assert_eq!(entry.credits, Some(3.25));
    }

    mod orchestration {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::mpsc::{Receiver, channel};

        use super::*;
        use crate::hooks::{self, Ask};
        use crate::model::NewAgent;
        use crate::orchestrator::engine::Call;
        use crate::orchestrator::runner::{MAX_ANSWERS_IN_A_ROW, SETTLE as FOLLOW_AFTER};
        use crate::orchestrator::{
            By, ChatRole, Decision, DecisionStatus, How, MAX_ENGINE_PROCESSES, Mode, OrchestratorView, Outcome,
            ProjectChange, SettingsChange, UserAnswer, instructions, relay,
        };
        use crate::protocol::{PROTOCOL_VERSION, Snapshot, TermSize, Welcome};
        use crate::pwa::actions;
        use crate::pwa::protocol::Tone;
        use crate::pwa::server::{Outbox, Peer};

        const QUESTION: &str = "Do you want to run the tests?";
        const SCREEN: &str = "$ agent\r\nDo you want to run the tests?\r\n\u{276f} 1. Yes\r\n  2. No\r\n\r\n";
        const PRESS_ONE: &str = r#"```json
{"action":"answer","keys":["1"],"text":null,"summary":"Allowed the tests","reason":"the task asks for them","tag":null,"deny":false}
```"#;
        const TYPE_YES: &str = r#"{"action":"answer","text":"yes","summary":"Told it to go on","reason":"safe"}"#;
        const ESCALATE: &str = r#"{"action":"escalate","keys":["2"],"summary":"Refuse the tests","reason":"they deploy","tag":"irreversible"}"#;
        const KILLED: &[u8] = b"<kill>";
        const WAIT: Duration = Duration::from_secs(60);
        const SETTLE: Duration = Duration::from_millis(300);

        type Script = Box<dyn Fn(AgentKind, &str) -> Result<String, String> + Send + Sync>;
        type Screens = Arc<dyn Fn(usize) -> String + Send + Sync>;

        struct FakeEngine {
            script: Script,
            delay: Duration,
            calls: Mutex<Vec<(AgentKind, Option<String>, String)>>,
            running: AtomicUsize,
            peak: AtomicUsize,
            stopped: Mutex<Vec<u64>>,
            stopped_all: AtomicUsize,
        }

        impl FakeEngine {
            fn calls(&self) -> Vec<(AgentKind, Option<String>, String)> {
                lock(&self.calls).clone()
            }
        }

        impl Engine for FakeEngine {
            fn ask(&self, _config: &Config, call: &Call) -> Result<String, String> {
                let running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(running, Ordering::SeqCst);
                lock(&self.calls).push((call.kind, call.model.map(str::to_string), call.prompt.to_string()));
                thread::sleep(self.delay);
                self.running.fetch_sub(1, Ordering::SeqCst);
                (self.script)(call.kind, call.prompt)
            }

            fn stop(&self, ticket: u64) {
                lock(&self.stopped).push(ticket);
            }

            fn stop_all(&self) {
                self.stopped_all.fetch_add(1, Ordering::SeqCst);
            }
        }

        struct Fixture {
            dir: PathBuf,
            store: Store,
            hub: Arc<Hub>,
            engine: Arc<FakeEngine>,
            window: Receiver<Event>,
            reply: Sender<Event>,
            replies: Receiver<Event>,
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }

        fn fixture_with(tag: &str, delay: Duration, script: Script) -> Fixture {
            let dir = std::env::temp_dir().join(format!("orkai-hub-octo-{tag}-{}-{}", std::process::id(), now_ms()));
            let store = Store::open(Home::at(dir.join("home"))).unwrap();
            let launcher = Launcher::new(store.clone(), Config::default(), &std::env::current_exe().unwrap()).unwrap();
            let engine = Arc::new(FakeEngine {
                script,
                delay,
                calls: Mutex::new(Vec::new()),
                running: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
                stopped: Mutex::new(Vec::new()),
                stopped_all: AtomicUsize::new(0),
            });
            let hub = Hub::orchestrating(launcher, "host".into(), Arc::clone(&engine) as Arc<dyn Engine>);
            let (tx, window) = channel();
            hub.inner().phones.insert(1, tx);
            let (reply, replies) = channel();
            Fixture { dir, store, hub, engine, window, reply, replies }
        }

        fn fixture(tag: &str, answer: &'static str) -> Fixture {
            fixture_with(tag, Duration::ZERO, Box::new(move |_, _| Ok(answer.to_string())))
        }

        impl Fixture {
            fn octo(&self) -> &Arc<Orchestrator> {
                self.hub.orchestrator().unwrap()
            }

            fn set(&self, change: SettingsChange) {
                self.octo().set(&self.hub, change, &self.reply);
            }

            fn enable(&self, mode: Mode) {
                self.set(SettingsChange {
                    enabled: Some(true),
                    mode: Some(mode),
                    primary: Some(Some(AgentKind::Claude)),
                    ..SettingsChange::default()
                });
            }

            fn paused(&self, name: &str, kind: AgentKind, since: u64) -> AgentId {
                let draft = NewAgent {
                    session: "default".into(),
                    name: name.into(),
                    kind,
                    cwd: self.dir.join("project"),
                    task: format!("task of {name}"),
                };
                let record = self.store.create_agent(draft).unwrap();
                let status = StatusReport {
                    state: AgentState::Paused,
                    state_since_ms: since,
                    ..report_of_a_run(1, AgentState::Paused)
                };
                let id = record.id;
                self.hub.inner().agents.insert(id, tracked_agent(record, Link::Pending, Some(status)));
                id
            }

            fn agent(&self, name: &str, kind: AgentKind, since: u64) -> (AgentId, Receiver<Vec<u8>>) {
                self.agent_showing(name, kind, since, Arc::new(|_| SCREEN.to_string()))
            }

            fn agent_showing(
                &self,
                name: &str,
                kind: AgentKind,
                since: u64,
                screens: Screens,
            ) -> (AgentId, Receiver<Vec<u8>>) {
                let id = self.paused(name, kind, since);
                (id, fake_holder(&self.store, id, screens))
            }

            fn status(&self, id: AgentId, state: AgentState, since: u64) {
                let mut inner = self.hub.inner();
                let status = inner.agents.get_mut(&id).unwrap().status.as_mut().unwrap();
                (status.state, status.state_since_ms) = (state, since);
            }

            fn view(&self) -> OrchestratorView {
                self.octo().view()
            }

            fn decision_of(&self, agent: AgentId) -> Option<Decision> {
                self.view().decisions.into_iter().find(|decision| decision.agent == agent)
            }

            fn waits_for_you(&self, agent: AgentId) -> Decision {
                until("the decision waits for the user", || {
                    self.decision_of(agent).filter(|decision| decision.status == DecisionStatus::NeedsYou)
                })
            }

            fn broadcasts(&self) -> Vec<OrchestratorView> {
                let mut views = Vec::new();
                while let Ok(event) = self.window.try_recv() {
                    if let Event::Broadcast(bytes) = event
                        && let Ok(BridgeMsg::Orchestrator(view)) = serde_json::from_slice(&bytes)
                    {
                        views.push(view);
                    }
                }
                views
            }

            fn reply_where<T>(&self, wanted: impl Fn(BridgeMsg) -> Option<T>) -> T {
                let deadline = Instant::now() + WAIT;
                while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                    if let Ok(Event::Reply(message)) = self.replies.recv_timeout(left)
                        && let Some(found) = wanted(message)
                    {
                        return found;
                    }
                }
                panic!("the reply did not arrive in time");
            }

            fn notice(&self) -> (Tone, String) {
                self.reply_where(|message| match message {
                    BridgeMsg::Notice { text, tone } => Some((tone, text)),
                    _ => None,
                })
            }
        }

        fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
            let deadline = Instant::now() + WAIT;
            while Instant::now() < deadline {
                if let Some(found) = check() {
                    return found;
                }
                thread::sleep(Duration::from_millis(10));
            }
            panic!("{what}: it did not happen in time");
        }

        fn fake_holder(store: &Store, id: AgentId, screens: Screens) -> Receiver<Vec<u8>> {
            let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            let runtime = RuntimeInfo {
                holder_pid: 1,
                child_pid: None,
                port: listener.local_addr().unwrap().port(),
                token: "test".into(),
                started_at_ms: 1,
                protocol: PROTOCOL_VERSION,
                version: "test".into(),
                exe: None,
            };
            crate::fsutil::write_json_atomic(&store.home().runtime_file(id), &runtime).unwrap();
            let (typed, received) = channel();
            let inputs = Arc::new(AtomicUsize::new(0));
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { return };
                    let (typed, inputs, screens) = (typed.clone(), Arc::clone(&inputs), Arc::clone(&screens));
                    thread::spawn(move || {
                        if ClientMsg::read(&mut stream).is_err() {
                            return;
                        }
                        let welcome =
                            Welcome { agent_id: id, protocol: PROTOCOL_VERSION, version: "test".into(), holder_pid: 1 };
                        let _ = std::io::Write::write_all(&mut stream, &HolderMsg::Welcome(welcome).to_frame());
                        while let Ok(message) = ClientMsg::read(&mut stream) {
                            match message {
                                ClientMsg::Peek => {
                                    let data = screens(inputs.load(Ordering::SeqCst)).into_bytes();
                                    let screen = Snapshot { size: TermSize::new(24, 80), data };
                                    let _ =
                                        std::io::Write::write_all(&mut stream, &HolderMsg::Snapshot(screen).to_frame());
                                }
                                ClientMsg::Input(bytes) => {
                                    inputs.fetch_add(1, Ordering::SeqCst);
                                    let _ = typed.send(bytes);
                                }
                                ClientMsg::Kill => {
                                    let _ = typed.send(KILLED.to_vec());
                                }
                                _ => {}
                            }
                        }
                    });
                }
            });
            received
        }

        fn typed_into(holder: &Receiver<Vec<u8>>, count: usize) -> Vec<Vec<u8>> {
            (0..count).map(|_| holder.recv_timeout(WAIT).expect("the agent was not typed into")).collect()
        }

        fn nothing_typed(holder: &Receiver<Vec<u8>>) {
            assert_eq!(holder.recv_timeout(SETTLE).ok(), None, "nothing may be typed into the agent");
        }

        fn sent(messages: &[&[u8]]) -> Vec<Vec<u8>> {
            messages.iter().map(|message| message.to_vec()).collect()
        }

        fn prompts_about(engine: &FakeEngine, name: &str) -> usize {
            engine.calls().iter().filter(|(_, _, prompt)| prompt.contains(&format!("Agent: {name} ("))).count()
        }

        #[test]
        fn an_agent_is_seen_as_paused_working_or_not_known_yet() {
            let dir = std::env::temp_dir().join(format!("orkai-hub-sight-{}-{}", std::process::id(), now_ms()));
            let (_store, record) = record_of(&dir, 2, 1);
            let paused = StatusReport { state_since_ms: 70, ..report_of_a_run(1, AgentState::Paused) };
            let sight = |link, status| tracked_agent(record.clone(), link, status).sighting();
            assert_eq!(sight(Link::Pending, Some(paused.clone())), (record.id, Sight::Paused(70)));
            assert_eq!(sight(Link::Pending, Some(report_of_a_run(1, AgentState::Working))), (record.id, Sight::Other));
            assert_eq!(
                sight(Link::Connecting, None),
                (record.id, Sight::Unknown),
                "nothing is decided before it reports"
            );
            assert_eq!(sight(Link::Lost(None), Some(paused.clone())).1, Sight::Other, "a dead agent asks nothing");
            let exit = ExitInfo { code: Some(0), error: None, at_ms: 1 };
            assert_eq!(sight(Link::Exited(exit), Some(paused)).1, Sight::Other);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn a_disabled_orchestrator_does_nothing_and_touches_no_file() {
            let fx = fixture("disabled", PRESS_ONE);
            let (_, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            assert!(!fx.octo().is_enabled());
            for _ in 0..3 {
                fx.hub.orchestrate(true);
            }
            thread::sleep(SETTLE);
            assert!(fx.engine.calls().is_empty(), "no engine runs");
            assert!(fx.broadcasts().is_empty(), "nothing is broadcast");
            assert!(fx.view().decisions.is_empty());
            let written: Vec<_> = std::fs::read_dir(fx.store.home().orchestrator_dir()).unwrap().flatten().collect();
            assert_eq!(written.len(), 1, "only the lock that says the desktop is here exists: {written:?}");
            nothing_typed(&holder);
        }

        #[test]
        fn observe_opens_a_decision_with_what_octo_would_answer_and_never_types() {
            let fx = fixture("observe", PRESS_ONE);
            fx.enable(Mode::Observe);
            let (id, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            let decision = fx.waits_for_you(id);
            assert_eq!((decision.agent, decision.name.as_str(), decision.kind), (id, "alpha", AgentKind::Codex));
            assert_eq!((decision.session.as_str(), decision.since), ("default", 100));
            assert_eq!(decision.project, fx.hub.agent_view(id).unwrap().project, "the project the hub groups it under");
            assert_eq!(decision.question, QUESTION);
            assert_eq!(
                decision.context,
                ["$ agent", QUESTION, "\u{276f} 1. Yes", "  2. No"],
                "the screen without blank rows"
            );
            assert_eq!(decision.tool, None);
            let suggestion = decision.suggestion.expect("what Octo would answer");
            assert_eq!((suggestion.keys, suggestion.text), (Some(vec!["1".to_string()]), None));
            assert_eq!(suggestion.summary, "Allowed the tests");
            assert_eq!(decision.problem, None);
            nothing_typed(&holder);

            let calls = fx.engine.calls();
            assert_eq!(calls.len(), 1);
            assert_eq!((calls[0].0, calls[0].1.clone()), (AgentKind::Claude, None));
            assert!(calls[0].2.contains("Mode: observe.") && calls[0].2.contains(QUESTION), "{}", calls[0].2);
            assert!(calls[0].2.contains("Task: task of alpha"));
            let logged = fx.view().handled;
            assert_eq!(
                (logged[0].by, logged[0].outcome, logged[0].mode),
                (By::Octo, Outcome::Escalated, Mode::Observe)
            );
            assert_eq!((logged[0].summary.as_str(), logged[0].question.as_str()), ("Allowed the tests", QUESTION));
            assert_eq!(fx.view().handled_today, 0, "a question left for you is not one Octo handled");

            fx.hub.orchestrate(true);
            assert_eq!(fx.engine.calls().len(), 1, "the same pause is judged once");
            fx.status(id, AgentState::Working, 200);
            fx.hub.orchestrate(true);
            assert!(fx.view().decisions.is_empty(), "the decision closes when the agent goes on");
            let closed = &fx.view().handled[0];
            assert_eq!(
                (closed.by, closed.outcome, closed.summary.as_str()),
                (By::You, Outcome::Answered, "Answered in the agent")
            );
            assert_eq!(fx.view().handled.len(), 2);
            nothing_typed(&holder);
        }

        #[test]
        fn a_new_pause_of_the_same_agent_is_a_new_decision() {
            let fx = fixture("again", ESCALATE);
            fx.enable(Mode::Moderate);
            let (id, _holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            let first = fx.waits_for_you(id);
            fx.status(id, AgentState::Paused, 900);
            fx.hub.orchestrate(true);
            let second = until("the new pause opens its own decision", || {
                fx.decision_of(id)
                    .filter(|decision| decision.since == 900 && decision.status == DecisionStatus::NeedsYou)
            });
            assert!(second.id > first.id);
            assert_eq!(fx.view().decisions.len(), 1);
            assert_eq!(fx.engine.calls().len(), 2);
        }

        #[test]
        fn an_agent_that_has_not_reported_yet_keeps_its_decision_and_a_removed_one_loses_it() {
            let fx = fixture("unknown", ESCALATE);
            fx.enable(Mode::Moderate);
            let (id, _holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            fx.waits_for_you(id);
            let status = fx.hub.inner().agents.get_mut(&id).unwrap().status.take();
            fx.hub.orchestrate(true);
            assert!(fx.decision_of(id).is_some(), "a window that just reopened has not heard from the agent yet");
            fx.hub.inner().agents.get_mut(&id).unwrap().status = status;
            fx.hub.orchestrate(true);
            assert_eq!(fx.engine.calls().len(), 1, "and the same pause is not judged again");
            let logged = fx.view().handled.len();
            fx.hub.inner().agents.remove(&id);
            fx.hub.orchestrate(true);
            assert!(fx.view().decisions.is_empty());
            assert_eq!(fx.view().handled.len(), logged, "an agent that was removed answered nothing");
        }

        #[test]
        fn moderate_answers_what_is_settled_and_leaves_what_is_open() {
            let fx = fixture_with(
                "moderate",
                Duration::ZERO,
                Box::new(|_, prompt| {
                    let answer = if prompt.contains("Agent: alpha (") { PRESS_ONE } else { ESCALATE };
                    Ok(answer.to_string())
                }),
            );
            fx.enable(Mode::Moderate);
            let (alpha, alpha_holder) = fx.agent("alpha", AgentKind::Codex, 100);
            let (beta, beta_holder) = fx.agent("beta", AgentKind::Kimi, 100);
            fx.hub.orchestrate(true);
            assert_eq!(typed_into(&alpha_holder, 1), sent(&[b"1"]), "the answer reaches the agent");
            let escalated = fx.waits_for_you(beta);
            assert_eq!(escalated.tag.as_deref(), Some("irreversible"));
            assert_eq!(escalated.suggestion.unwrap().keys, Some(vec!["2".to_string()]), "with what Octo would do");
            nothing_typed(&beta_holder);
            until("the answer is logged", || fx.view().handled.iter().any(|entry| entry.agent == alpha).then_some(()));
            assert_eq!(fx.decision_of(alpha), None, "an answered decision leaves the queue");
            let view = fx.view();
            let of = |agent| view.handled.iter().find(|entry| entry.agent == agent).unwrap();
            assert_eq!(
                (of(alpha).by, of(alpha).outcome, of(alpha).mode),
                (By::Octo, Outcome::Answered, Mode::Moderate)
            );
            assert_eq!(of(alpha).summary, "Allowed the tests");
            assert_eq!((of(beta).by, of(beta).outcome), (By::Octo, Outcome::Escalated));
            assert_eq!(view.handled_today, 1);
            assert_eq!(view.handled_by_project.len(), 1);
            assert_eq!(
                (view.handled_by_project[0].path.as_str(), view.handled_by_project[0].count),
                (of(alpha).project.as_str(), 1)
            );

            fx.hub.orchestrate(true);
            thread::sleep(SETTLE);
            assert_eq!(
                prompts_about(&fx.engine, "alpha"),
                1,
                "a pause Octo answered is not judged again while it lasts"
            );
            nothing_typed(&alpha_holder);
            let journal = fx.store.home().orchestrator_dir().join("activity.jsonl");
            until("both entries are on disk", || {
                std::fs::read_to_string(&journal).ok().filter(|text| text.lines().count() == 2)
            });
        }

        #[test]
        fn a_project_in_observe_is_never_answered_even_when_the_default_mode_answers() {
            let fx = fixture("project-mode", PRESS_ONE);
            fx.enable(Mode::Bypass);
            let (id, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            let project = fx.hub.agent_view(id).unwrap().project;
            fx.set(SettingsChange {
                project: Some(ProjectChange { path: project, mode: Some(Mode::Observe) }),
                ..SettingsChange::default()
            });
            fx.hub.orchestrate(true);
            fx.waits_for_you(id);
            assert!(fx.engine.calls()[0].2.contains("Mode: observe."));
            nothing_typed(&holder);
        }

        #[test]
        fn bypass_allows_a_claude_permission_by_rule_and_asks_the_engine_for_everything_else() {
            let fx = fixture("bypass", TYPE_YES);
            fx.enable(Mode::Bypass);
            let (claude, claude_holder) = fx.agent("alpha", AgentKind::Claude, 100);
            let asked = Ask { tool: "Bash".into(), detail: "npm test\nRun the suite".into(), at_ms: 100 };
            hooks::write_ask(fx.store.home(), claude, &asked).unwrap();
            let (codex, codex_holder) = fx.agent("beta", AgentKind::Codex, 100);
            let (plan, plan_holder) = fx.agent("gamma", AgentKind::Claude, 100);
            let planned = Ask { tool: "ExitPlanMode".into(), detail: "1. write the tests".into(), at_ms: 100 };
            hooks::write_ask(fx.store.home(), plan, &planned).unwrap();
            fx.hub.orchestrate(true);

            assert_eq!(typed_into(&claude_holder, 1), sent(&[b"\r"]), "a permission is allowed with Enter");
            assert_eq!(typed_into(&codex_holder, 2), sent(&[b"yes", b"\r"]), "text is typed and then submitted");
            assert_eq!(typed_into(&plan_holder, 2), sent(&[b"yes", b"\r"]), "a question tool goes to the engine");
            until("the three answers are logged", || (fx.view().handled.len() == 3).then_some(()));
            let view = fx.view();
            let of = |agent| view.handled.iter().find(|entry| entry.agent == agent).unwrap();
            assert_eq!((of(claude).by, of(claude).outcome), (By::Rule, Outcome::Answered));
            assert_eq!(
                (of(claude).summary.as_str(), of(claude).question.as_str()),
                ("Allowed Bash: npm test", "npm test")
            );
            assert_eq!((of(codex).by, of(codex).summary.as_str()), (By::Octo, "Told it to go on"));
            assert_eq!(of(plan).question, "1. write the tests");
            assert_eq!(prompts_about(&fx.engine, "alpha"), 0, "the rule needs no engine");
            assert_eq!(fx.engine.calls().len(), 2);
            let plan_prompt = fx.engine.calls().into_iter().find(|call| call.2.contains("Agent: gamma (")).unwrap().2;
            assert!(
                plan_prompt.contains("Tool waiting for permission (ExitPlanMode):\n1. write the tests"),
                "{plan_prompt}"
            );
            assert!(plan_prompt.contains("Mode: bypass."));
            assert!(view.decisions.is_empty());
            assert_eq!(view.handled_today, 3);
        }

        #[test]
        fn an_ask_left_by_an_earlier_pause_is_not_taken_for_this_one() {
            let fx = fixture("stale-ask", ESCALATE);
            fx.enable(Mode::Bypass);
            let (id, holder) = fx.agent("alpha", AgentKind::Claude, 500);
            let old = Ask { tool: "Bash".into(), detail: "rm -rf build".into(), at_ms: 499 };
            hooks::write_ask(fx.store.home(), id, &old).unwrap();
            fx.hub.orchestrate(true);
            let decision = fx.waits_for_you(id);
            assert_eq!(decision.tool, None);
            assert_eq!(decision.question, QUESTION, "the screen is what says what it asks now");
            assert!(
                decision.problem.unwrap().starts_with("Octo could not decide"),
                "bypass that escalates waits with why"
            );
            nothing_typed(&holder);
        }

        #[test]
        fn an_answer_for_a_pause_that_already_ended_is_dropped() {
            let fx = fixture_with("stale", Duration::from_millis(400), Box::new(|_, _| Ok(PRESS_ONE.to_string())));
            fx.enable(Mode::Moderate);
            let (id, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            until("the engine is asked", || (fx.engine.calls().len() == 1).then_some(()));
            fx.status(id, AgentState::Paused, 700);
            until("the stale decision is dropped", || fx.decision_of(id).is_none().then_some(()));
            nothing_typed(&holder);
            assert!(fx.view().handled.is_empty(), "an answer that was never sent is not logged as sent");
            fx.hub.orchestrate(true);
            assert_eq!(typed_into(&holder, 1), sent(&[b"1"]), "the new pause gets its own answer");
            assert_eq!(fx.engine.calls().len(), 2);
        }

        #[test]
        fn switching_to_observe_while_the_engine_thinks_keeps_its_answer_from_being_typed() {
            let fx = fixture_with("switch", Duration::from_millis(400), Box::new(|_, _| Ok(PRESS_ONE.to_string())));
            fx.enable(Mode::Moderate);
            let (id, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            until("the engine is asked", || (fx.engine.calls().len() == 1).then_some(()));
            fx.set(SettingsChange { mode: Some(Mode::Observe), ..SettingsChange::default() });
            let decision = fx.waits_for_you(id);
            assert!(decision.suggestion.is_some());
            nothing_typed(&holder);
        }

        #[test]
        fn an_engine_that_fails_leaves_the_decision_with_the_problem_and_the_secondary_is_tried_once() {
            let fx = fixture_with(
                "failure",
                Duration::ZERO,
                Box::new(|kind, _| match kind {
                    AgentKind::Claude => Err("claude: Please run /login".to_string()),
                    _ => Ok(PRESS_ONE.to_string()),
                }),
            );
            fx.enable(Mode::Moderate);
            let (alpha, alpha_holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            let failed = fx.waits_for_you(alpha);
            assert_eq!(failed.problem.as_deref(), Some("claude: Please run /login"));
            assert_eq!(failed.suggestion, None);
            assert_eq!(fx.view().problem.as_deref(), Some("claude: Please run /login"));
            let logged = &fx.view().handled[0];
            assert_eq!(
                (logged.by, logged.outcome, logged.summary.as_str()),
                (By::Octo, Outcome::Failed, "claude: Please run /login")
            );
            assert_eq!(fx.engine.calls().len(), 1);
            nothing_typed(&alpha_holder);

            fx.set(SettingsChange {
                secondary: Some(Some(AgentKind::Codex)),
                secondary_model: Some(Some("gpt-6".into())),
                ..SettingsChange::default()
            });
            let (beta, beta_holder) = fx.agent("beta", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            assert_eq!(typed_into(&beta_holder, 1), sent(&[b"1"]), "the secondary engine answered");
            let calls = fx.engine.calls();
            let about_beta: Vec<_> = calls.iter().filter(|call| call.2.contains("Agent: beta (")).collect();
            assert_eq!(
                about_beta.iter().map(|call| (call.0, call.1.as_deref())).collect::<Vec<_>>(),
                [(AgentKind::Claude, None), (AgentKind::Codex, Some("gpt-6"))]
            );
            assert_eq!(about_beta[0].2, about_beta[1].2, "with the same prompt");
            until("the problem is cleared", || fx.view().problem.is_none().then_some(()));
            assert_eq!(fx.decision_of(beta), None);
            assert!(fx.decision_of(alpha).is_some(), "a decision that failed is not judged again by itself");
        }

        #[test]
        fn both_engines_failing_or_an_answer_in_no_format_or_no_engine_chosen_all_wait_for_you() {
            let fx = fixture_with(
                "no-format",
                Duration::ZERO,
                Box::new(|kind, _| match kind {
                    AgentKind::Claude => Ok("I think you should allow it.".to_string()),
                    _ => Err("codex gave no answer".to_string()),
                }),
            );
            fx.set(SettingsChange { enabled: Some(true), mode: Some(Mode::Bypass), ..SettingsChange::default() });
            let (alpha, _alpha_holder) = fx.agent("alpha", AgentKind::Kimi, 100);
            fx.hub.orchestrate(true);
            assert_eq!(fx.waits_for_you(alpha).problem.as_deref(), Some("choose an engine for Octo in its settings"));
            assert!(fx.engine.calls().is_empty());

            fx.set(SettingsChange {
                primary: Some(Some(AgentKind::Claude)),
                secondary: Some(Some(AgentKind::Codex)),
                ..SettingsChange::default()
            });
            let (beta, beta_holder) = fx.agent("beta", AgentKind::Kimi, 100);
            fx.hub.orchestrate(true);
            assert_eq!(
                fx.waits_for_you(beta).problem.as_deref(),
                Some("claude did not answer in the expected format; codex gave no answer")
            );
            assert_eq!(fx.engine.calls().len(), 2);
            nothing_typed(&beta_holder);
        }

        #[test]
        fn the_decisions_are_bounded_and_a_paused_agent_beyond_the_limit_waits_for_room() {
            use crate::orchestrator::MAX_DECISIONS;
            let fx = fixture("bounded", ESCALATE);
            fx.enable(Mode::Observe);
            let agents: Vec<AgentId> = (0..MAX_DECISIONS + 2)
                .map(|index| fx.paused(&format!("agent-{index}"), AgentKind::Codex, 100))
                .collect();
            fx.hub.orchestrate(true);
            assert_eq!(fx.view().decisions.len(), MAX_DECISIONS);
            let last = *agents.last().unwrap();
            assert_eq!(fx.decision_of(last), None, "the agents beyond the limit have no decision yet");
            fx.hub.orchestrate(true);
            assert_eq!(fx.view().decisions.len(), MAX_DECISIONS, "another tick does not grow the queue");
            for agent in &agents[..2] {
                fx.waits_for_you(*agent);
                fx.status(*agent, AgentState::Working, 200);
            }
            fx.hub.orchestrate(true);
            assert_eq!(fx.view().decisions.len(), MAX_DECISIONS);
            assert!(fx.decision_of(last).is_some(), "room made by an agent that went on is taken");
            until("every decision is judged once", || (fx.engine.calls().len() == MAX_DECISIONS + 2).then_some(()));
            assert!(fx.engine.peak.load(Ordering::SeqCst) <= MAX_ENGINE_PROCESSES);
        }

        #[test]
        fn ten_paused_agents_never_run_more_than_two_engines_at_once() {
            let fx = fixture_with("queue", Duration::from_millis(60), Box::new(|_, _| Ok(ESCALATE.to_string())));
            fx.enable(Mode::Moderate);
            let agents: Vec<(AgentId, Receiver<Vec<u8>>)> =
                (0..10).map(|index| fx.agent(&format!("agent-{index}"), AgentKind::Codex, 100)).collect();
            fx.hub.orchestrate(true);
            until("every question is judged", || {
                let waiting = fx.view().decisions.iter().filter(|d| d.status == DecisionStatus::NeedsYou).count();
                (waiting == agents.len()).then_some(())
            });
            assert_eq!(fx.engine.calls().len(), agents.len(), "each question is judged once");
            assert_eq!(fx.engine.peak.load(Ordering::SeqCst), MAX_ENGINE_PROCESSES, "the others wait in the queue");
            let decisions = fx.view().decisions;
            assert!(decisions.windows(2).all(|pair| pair[0].id < pair[1].id), "oldest first");
            assert!(decisions.iter().all(|decision| decision.question == QUESTION));
        }

        #[test]
        fn a_change_of_the_view_is_broadcast_once() {
            let fx = fixture("broadcast", ESCALATE);
            fx.enable(Mode::Moderate);
            let enabled = fx.broadcasts();
            assert_eq!(enabled.len(), 1);
            assert!(
                enabled[0].enabled
                    && enabled[0].mode == Mode::Moderate
                    && enabled[0].primary == Some(AgentKind::Claude)
            );
            assert_eq!(
                enabled[0].engines.iter().map(|engine| engine.kind).collect::<Vec<_>>(),
                [AgentKind::Claude, AgentKind::Codex]
            );
            fx.enable(Mode::Moderate);
            assert!(fx.broadcasts().is_empty(), "settings that did not change say nothing");

            let (id, _holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            fx.waits_for_you(id);
            let views = fx.broadcasts();
            let stages: Vec<(DecisionStatus, bool)> =
                views.iter().map(|view| (view.decisions[0].status, view.decisions[0].question.is_empty())).collect();
            assert_eq!(
                stages,
                [
                    (DecisionStatus::Deciding, true),
                    (DecisionStatus::Deciding, false),
                    (DecisionStatus::NeedsYou, false)
                ],
                "opened, read, judged"
            );
            assert_eq!(views[2], fx.view(), "the last broadcast is the current view");
            for _ in 0..3 {
                fx.hub.orchestrate(true);
            }
            thread::sleep(SETTLE);
            assert!(fx.broadcasts().is_empty(), "a tick that changes nothing broadcasts nothing");

            fx.octo().show(&fx.hub, &fx.reply);
            let shown = fx.reply_where(|message| match message {
                BridgeMsg::Orchestrator(view) => Some(view),
                _ => None,
            });
            assert_eq!(shown, fx.view(), "asking for the view answers only the one who asked");
            assert!(fx.broadcasts().is_empty());
        }

        #[test]
        fn turning_octo_off_drops_the_decisions_and_keeps_the_choice_for_the_next_run() {
            let fx = fixture("off", ESCALATE);
            fx.enable(Mode::Moderate);
            let (id, _holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            fx.waits_for_you(id);
            fx.set(SettingsChange { enabled: Some(false), ..SettingsChange::default() });
            assert!(!fx.octo().is_enabled());
            assert!(fx.view().decisions.is_empty());
            fx.hub.orchestrate(true);
            thread::sleep(SETTLE);
            assert_eq!(fx.engine.calls().len(), 1);
            let reopened = Orchestrator::open(fx.store.home(), Arc::clone(&fx.engine) as Arc<dyn Engine>);
            assert!(!reopened.is_enabled());
            assert_eq!((reopened.view().mode, reopened.view().primary), (Mode::Moderate, Some(AgentKind::Claude)));
            assert_eq!(reopened.view().handled.len(), 0, "a disabled Octo reads no log when it opens");

            fx.set(SettingsChange { primary: Some(Some(AgentKind::Kimi)), ..SettingsChange::default() });
            let (tone, text) = fx.notice();
            assert_eq!(tone, Tone::Error);
            assert!(text.contains("kimi"), "{text}");
            assert_eq!(fx.view().primary, Some(AgentKind::Claude));
        }

        #[test]
        fn the_user_answers_a_decision_with_the_suggestion_with_text_with_keys_or_by_allowing_it() {
            let fx = fixture("you", ESCALATE);
            fx.enable(Mode::Observe);
            let answer = |decision: u64, how, text: Option<&str>, keys: Option<&[&str]>, remember: Option<&str>| {
                let answer = UserAnswer {
                    decision,
                    how,
                    text: text.map(str::to_string),
                    keys: keys.map(|keys| keys.iter().map(|key| (*key).to_string()).collect()),
                    remember: remember.map(str::to_string),
                };
                fx.octo().answer_from_user(&fx.hub, answer, &fx.reply);
            };
            let logged = |agent: AgentId| {
                until("the answer of the user is logged", || {
                    fx.view().handled.into_iter().find(|entry| entry.agent == agent && entry.by == By::You)
                })
            };
            let waiting = |name: &str| {
                let (id, holder) = fx.agent(name, AgentKind::Codex, 100);
                fx.hub.orchestrate(true);
                (id, fx.waits_for_you(id), holder)
            };

            let (alpha, decision, holder) = waiting("alpha");
            answer(decision.id, How::Suggestion, None, None, None);
            assert_eq!(typed_into(&holder, 1), sent(&[b"2"]));
            assert_eq!(
                (logged(alpha).outcome, logged(alpha).summary),
                (Outcome::Answered, "Refuse the tests".to_string())
            );
            assert_eq!(fx.decision_of(alpha), None);

            let (beta, decision, holder) = waiting("beta");
            answer(decision.id, How::Text, Some(" keep the old endpoint "), None, Some(" always keep\nold endpoints "));
            assert_eq!(typed_into(&holder, 2), sent(&[b"keep the old endpoint", b"\r"]));
            assert_eq!(logged(beta).summary, "Typed: keep the old endpoint");
            assert_eq!(
                instructions::read(fx.store.home(), Some(&decision.project)).unwrap(),
                "- always keep old endpoints\n",
                "the rule is remembered for the project"
            );

            let (gamma, decision, holder) = waiting("gamma");
            answer(decision.id, How::Keys, None, Some(&["ArrowDown", "Enter"]), None);
            assert_eq!(typed_into(&holder, 2), sent(&[b"\x1b[B", b"\r"]), "each key is its own keystroke");
            assert_eq!(logged(gamma).summary, "Pressed ArrowDown Enter");

            let (delta, decision, holder) = waiting("delta");
            answer(decision.id, How::Keys, None, Some(&["Bogus"]), None);
            assert_eq!(fx.notice(), (Tone::Warn, "those keys cannot be pressed".to_string()));
            answer(decision.id, How::Text, Some("  "), None, None);
            assert_eq!(fx.notice(), (Tone::Warn, "write what to answer".to_string()));
            assert!(fx.decision_of(delta).is_some(), "an answer that says nothing leaves the decision waiting");
            answer(decision.id, How::Allow, None, None, None);
            assert_eq!(typed_into(&holder, 1), sent(&[b"\r"]));
            assert_eq!(logged(delta).summary, "Allowed it");

            let (epsilon, decision, holder) = waiting("epsilon");
            answer(decision.id, How::Dismiss, None, None, Some("ask me about deploys"));
            until("the dismissed decision leaves the queue", || fx.decision_of(epsilon).is_none().then_some(()));
            nothing_typed(&holder);
            until("the rule of a dismissed decision is remembered", || {
                let rules = instructions::read(fx.store.home(), Some(&decision.project)).unwrap();
                rules.ends_with("- ask me about deploys\n").then_some(())
            });
            fx.hub.orchestrate(true);
            thread::sleep(SETTLE);
            assert_eq!(fx.decision_of(epsilon), None, "a dismissed pause does not come back while it lasts");
            assert_eq!(prompts_about(&fx.engine, "epsilon"), 1);
            answer(decision.id, How::Allow, None, None, None);
            assert_eq!(fx.notice(), (Tone::Warn, "that decision is no longer pending".to_string()));
            answer(999, How::Allow, None, None, None);
            assert_eq!(fx.notice().0, Tone::Warn);

            let (zeta, decision, holder) = waiting("zeta");
            fx.status(zeta, AgentState::Working, 300);
            answer(decision.id, How::Allow, None, None, None);
            assert_eq!(fx.notice(), (Tone::Info, "the agent had already moved on".to_string()));
            nothing_typed(&holder);
            assert_eq!(fx.decision_of(zeta), None);
        }

        #[test]
        fn an_answer_that_cannot_reach_the_agent_stays_with_the_user_and_says_why() {
            let fx = fixture("unreachable", PRESS_ONE);
            fx.enable(Mode::Moderate);
            let (id, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            drop(holder);
            std::fs::remove_file(fx.store.home().runtime_file(id)).unwrap();
            fx.hub.orchestrate(true);
            let decision = fx.waits_for_you(id);
            assert!(decision.problem.unwrap().starts_with("the answer did not reach the agent"), "the send failed");
            assert!(decision.context.is_empty(), "a screen that cannot be read is no context");
            assert_eq!(
                decision.suggestion.unwrap().keys,
                Some(vec!["1".to_string()]),
                "what Octo wanted to answer stays"
            );
            assert!(fx.view().handled.is_empty());
        }

        const CHAT_PROMPT: &str = "The user is talking to you";
        const SECOND_ROUND: &str = "you cannot ask to read again";

        fn id_on_board<'a>(prompt: &'a str, name: &str) -> &'a str {
            let line = prompt.lines().find(|line| line.starts_with('#') && line.contains(&format!(" {name} (")));
            line.and_then(|line| line.trim_start_matches('#').split_whitespace().next()).unwrap_or("0")
        }

        fn chat_script() -> Script {
            Box::new(|_, prompt| {
                if !prompt.contains(CHAT_PROMPT) {
                    return Ok(ESCALATE.to_string());
                }
                if prompt.contains("The user says:\nplease fail") {
                    return Err("claude: boom".to_string());
                }
                let (alpha, gamma) = (id_on_board(prompt, "alpha"), id_on_board(prompt, "gamma"));
                let pending = prompt.lines().find(|line| line.starts_with("decision ") && line.contains(" beta "));
                let decision = pending.and_then(|line| line.split_whitespace().nth(1)).unwrap_or("0");
                if !prompt.contains(SECOND_ROUND) {
                    return Ok(format!(r#"{{"reply":"let me look","read":[{alpha}]}}"#));
                }
                Ok(format!(
                    r#"Here it is:
{{"reply":"Done.","read":[{alpha}],"actions":[
 {{"type":"send","agent":{alpha},"text":"run the tests"}},
 {{"type":"answer","decision":{decision},"keys":["1"]}},
 {{"type":"answer","decision":{decision},"text":"again"}},
 {{"type":"stop","agent":{gamma}}},
 {{"type":"start","kind":"claude","cwd":"~","task":"fix it","session":null}},
 {{"type":"start","kind":"notepad","cwd":"~"}},
 {{"type":"send","agent":999,"text":"hello"}},
 {{"type":"dance"}}
]}}"#
                ))
            })
        }

        #[test]
        fn a_chat_turn_reads_the_screens_it_asks_for_once_and_runs_each_action() {
            let fx = fixture_with("chat", Duration::from_millis(150), chat_script());
            let missing = Config { claude_command: "orkai-definitely-missing-agent".into(), ..Config::default() };
            fx.hub.reconfigure(missing).unwrap();
            fx.enable(Mode::Observe);
            let (alpha, alpha_holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.status(alpha, AgentState::Working, 100);
            let (beta, beta_holder) = fx.agent("beta", AgentKind::Codex, 100);
            let (gamma, gamma_holder) = fx.agent("gamma", AgentKind::Codex, 100);
            fx.status(gamma, AgentState::Idle, 100);
            fx.hub.orchestrate(true);
            let pending = fx.waits_for_you(beta);
            fx.broadcasts();

            fx.octo().chat(&fx.hub, "  what is alpha doing?  ", &fx.reply);
            let asked = fx.view();
            assert!(asked.thinking);
            assert_eq!(asked.chat.len(), 1);
            assert_eq!((asked.chat[0].role, asked.chat[0].text.as_str()), (ChatRole::You, "what is alpha doing?"));
            fx.octo().chat(&fx.hub, "and beta?", &fx.reply);
            assert_eq!(fx.notice(), (Tone::Warn, "Octo is still answering your last message".to_string()));
            fx.octo().chat(&fx.hub, "   ", &fx.reply);
            let view = until("Octo answers", || Some(fx.view()).filter(|view| !view.thinking));

            assert_eq!(view.chat.len(), 2, "one turn at a time");
            let said = &view.chat[1];
            assert_eq!((said.role, said.text.as_str(), said.failed), (ChatRole::Octo, "Done.", false));
            assert_eq!(
                said.actions,
                [
                    "Sent to alpha: run the tests".to_string(),
                    format!("Answered decision {}: Pressed 1", pending.id),
                    format!("Decision {} is no longer pending", pending.id),
                    "Stopping gamma".to_string(),
                    "Could not start claude: claude is not installed on host".to_string(),
                    "Could not start notepad: it is not an agent Octo knows".to_string(),
                    "Could not send to #999: it is not running".to_string(),
                    "Skipped an action Octo wrote wrongly".to_string(),
                ]
            );
            assert_eq!(typed_into(&alpha_holder, 2), sent(&[b"run the tests", b"\r"]));
            assert_eq!(typed_into(&beta_holder, 1), sent(&[b"1"]));
            assert_eq!(typed_into(&gamma_holder, 1), sent(&[KILLED]));
            assert_eq!(fx.decision_of(beta), None);
            let logged =
                view.handled.iter().find(|entry| entry.agent == beta && entry.outcome == Outcome::Answered).unwrap();
            assert_eq!((logged.by, logged.summary.as_str()), (By::Octo, "Pressed 1"));

            let turns: Vec<String> = fx
                .engine
                .calls()
                .into_iter()
                .map(|call| call.2)
                .filter(|prompt| prompt.contains(CHAT_PROMPT))
                .collect();
            assert_eq!(turns.len(), 2, "the screens are read in one extra round and no more");
            let board: Vec<&str> = turns[0].lines().filter(|line| line.contains(" (codex) project ")).collect();
            assert_eq!(board.len(), 3);
            assert!(board[0].starts_with(&format!("#{beta} beta (codex)")) && board[0].contains("paused"), "{board:?}");
            assert!(turns[0].contains(&format!("decision {} — agent #{beta} beta — waiting for the user", pending.id)));
            assert!(turns[0].contains("The user says:\nwhat is alpha doing?") && !turns[0].contains(SECOND_ROUND));
            assert!(!turns[0].contains("User: what is alpha doing?"), "the message is not repeated as history");
            let screen = format!("Screen of agent #{alpha}, last rows:\n$ agent\n{QUESTION}");
            assert!(turns[1].starts_with(&turns[0]) && turns[1].contains(&screen), "{}", turns[1]);
            let pushed = fx.broadcasts();
            assert!(
                pushed.first().is_some_and(|view| view.thinking && view.chat.len() == 1),
                "the window sees it thinking"
            );
            assert!(pushed.last().is_some_and(|view| !view.thinking && view.chat.len() == 2));

            fx.octo().chat(&fx.hub, "please fail", &fx.reply);
            let view =
                until("the failure is told", || Some(fx.view()).filter(|view| !view.thinking && view.chat.len() == 4));
            assert_eq!((view.chat[3].text.as_str(), view.chat[3].failed), ("claude: boom", true));
            assert!(view.chat[3].actions.is_empty());
            assert_eq!(view.problem.as_deref(), Some("claude: boom"));
            let history = fx.engine.calls().pop().unwrap().2;
            assert!(history.contains("User: what is alpha doing?\nOcto: Done."), "{history}");

            let journal = fx.store.home().orchestrator_dir().join("chat.jsonl");
            until("the conversation is on disk", || {
                std::fs::read_to_string(&journal).ok().filter(|text| text.lines().count() == 4)
            });
            fx.octo().clear_chat(&fx.hub);
            assert!(fx.view().chat.is_empty());
            until("the cleared conversation leaves the disk", || (!journal.exists()).then_some(()));
        }

        #[test]
        fn from_the_chat_octo_renames_removes_and_restarts_agents_and_says_what_really_happened() {
            let fx = fixture_with(
                "crud",
                Duration::ZERO,
                Box::new(|_, prompt| {
                    if !prompt.contains(CHAT_PROMPT) {
                        return Ok(ESCALATE.to_string());
                    }
                    let (alpha, gamma) = (id_on_board(prompt, "alpha"), id_on_board(prompt, "gamma"));
                    Ok(format!(
                        r#"{{"reply":"On it.","actions":[
 {{"type":"rename","agent":{alpha},"name":"   "}},
 {{"type":"remove","agent":{alpha}}},
 {{"type":"restart","agent":{alpha}}},
 {{"type":"remove","agent":{gamma}}},
 {{"type":"remove","agent":999}},
 {{"type":"restart","agent":999}},
 {{"type":"rename","agent":999,"name":"x"}},
 {{"type":"rename","agent":{alpha},"name":"  api  "}}
]}}"#
                    ))
                }),
            );
            fx.enable(Mode::Observe);
            let (alpha, _alpha_holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.status(alpha, AgentState::Working, 100);
            let (gamma, _gamma_holder) = fx.agent("gamma", AgentKind::Codex, 100);
            fx.status(gamma, AgentState::Exited, 100);

            fx.octo().chat(&fx.hub, "tidy up", &fx.reply);
            let view = until("Octo answers", || Some(fx.view()).filter(|view| !view.thinking && view.chat.len() == 2));
            assert_eq!(
                view.chat[1].actions,
                [
                    "Could not rename alpha: agent name cannot be empty",
                    "Could not remove alpha: stop it first",
                    "Could not restart alpha: it is still running",
                    "Removed gamma",
                    "Could not remove #999: there is no such agent",
                    "Could not restart #999: there is no such agent",
                    "Could not rename #999: there is no such agent",
                    "Renamed alpha to api",
                ]
            );
            assert_eq!(fx.store.load_agent(alpha).unwrap().name, "api");
            assert!(fx.store.load_agent(gamma).is_err(), "the stopped agent left the store");
            assert!(fx.store.load_agent(alpha).is_ok(), "a running agent is never removed");
        }

        #[test]
        fn octo_talks_even_when_it_is_not_watching_and_says_when_it_has_no_engine() {
            let fx =
                fixture_with("talk", Duration::ZERO, Box::new(|_, _| Ok("Nothing is running right now.".to_string())));
            fx.octo().chat(&fx.hub, "anything running?", &fx.reply);
            let view = until("Octo says it has no engine", || Some(fx.view()).filter(|view| view.chat.len() == 2));
            assert_eq!(
                (view.chat[1].text.as_str(), view.chat[1].failed),
                ("choose an engine for Octo in its settings", true)
            );
            assert!(fx.engine.calls().is_empty());
            fx.set(SettingsChange { primary: Some(Some(AgentKind::Codex)), ..SettingsChange::default() });
            fx.octo().chat(&fx.hub, "anything running?", &fx.reply);
            let view = until("Octo answers in prose", || Some(fx.view()).filter(|view| view.chat.len() == 4));
            assert_eq!((view.chat[3].text.as_str(), view.chat[3].failed), ("Nothing is running right now.", false));
            assert_eq!(view.problem, None);
            assert!(!view.enabled && !fx.octo().is_enabled(), "talking does not turn the watching on");
            assert_eq!(fx.engine.calls()[0].0, AgentKind::Codex);
            assert!(fx.engine.calls()[0].2.contains("Agents:\n(none)"));
        }

        #[test]
        fn the_window_reads_and_saves_the_instructions_and_pages_the_activity() {
            let fx = fixture("files", ESCALATE);
            let rules = |fx: &Fixture| {
                fx.reply_where(|message| match message {
                    BridgeMsg::OrchestratorInstructions { project, text, path } => Some((project, text, path)),
                    _ => None,
                })
            };
            actions::handle(&fx.hub, PhoneMsg::OrchestratorInstructions { project: None }, &fx.reply);
            let (project, text, path) = rules(&fx);
            assert_eq!((project, text.as_str()), (None, instructions::DEFAULT));
            assert_eq!(Path::new(&path), instructions::file(fx.store.home(), None));
            actions::handle(
                &fx.hub,
                PhoneMsg::OrchestratorInstructions { project: Some("dev/octo".into()) },
                &fx.reply,
            );
            assert_eq!(rules(&fx).1, "", "a project starts without rules");
            let save = |project: Option<&str>, text: String| {
                let project = project.map(str::to_string);
                actions::handle(&fx.hub, PhoneMsg::OrchestratorSaveInstructions { project, text }, &fx.reply);
            };
            save(Some("dev/octo"), "- use pnpm\n".into());
            let (project, text, path) = rules(&fx);
            assert_eq!((project.as_deref(), text.as_str()), (Some("dev/octo"), "- use pnpm\n"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "- use pnpm\n");
            save(None, "x".repeat(instructions::MAX_BYTES + 1));
            let (tone, refused) = fx.notice();
            assert_eq!(tone, Tone::Error);
            assert!(refused.contains("64 KiB"), "{refused}");

            fx.enable(Mode::Moderate);
            let agents: Vec<AgentId> =
                (0..3).map(|index| fx.agent(&format!("agent-{index}"), AgentKind::Codex, 100).0).collect();
            fx.hub.orchestrate(true);
            for agent in &agents {
                fx.waits_for_you(*agent);
            }
            let page = |before| {
                actions::handle(&fx.hub, PhoneMsg::OrchestratorActivity { before }, &fx.reply);
                fx.reply_where(|message| match message {
                    BridgeMsg::OrchestratorActivity { items, next } => Some((items, next)),
                    _ => None,
                })
            };
            let (items, next) =
                until("the three entries are on disk", || Some(page(None)).filter(|page| page.0.len() == 3));
            assert_eq!((items.iter().map(|item| item.seq).collect::<Vec<_>>(), next), (vec![3, 2, 1], None));
            assert!(items.iter().all(|item| item.outcome == Outcome::Escalated));
            assert_eq!(page(Some(3)).0.len(), 2);
            assert_eq!(page(Some(1)), (Vec::new(), None));
        }

        fn asks(step: usize) -> String {
            format!("$ agent\r\nStep {step}: continue?\r\n\u{276f} 1. Yes\r\n  2. No\r\n")
        }

        #[test]
        fn a_new_question_inside_the_same_pause_gets_its_own_decision() {
            let fx = fixture("follow", PRESS_ONE);
            fx.enable(Mode::Moderate);
            let shown = Arc::new(Mutex::new(SCREEN.to_string()));
            let showing = Arc::clone(&shown);
            let (id, holder) =
                fx.agent_showing("alpha", AgentKind::Codex, 100, Arc::new(move |_| lock(&showing).clone()));
            fx.hub.orchestrate(true);
            assert_eq!(typed_into(&holder, 1), sent(&[b"1"]));
            *lock(&shown) = asks(2);
            assert_eq!(typed_into(&holder, 1), sent(&[b"1"]), "the next question of the same pause is answered too");
            let calls = fx.engine.calls();
            assert_eq!(calls.len(), 2);
            assert!(calls[1].2.contains("Step 2: continue?") && !calls[1].2.contains(QUESTION), "{}", calls[1].2);
            let logged =
                until("both answers are logged", || Some(fx.view().handled).filter(|handled| handled.len() == 2));
            assert_eq!((logged[0].question.as_str(), logged[1].question.as_str()), ("Step 2: continue?", QUESTION));
            assert!(logged.iter().all(|entry| entry.agent == id && entry.by == By::Octo));
            thread::sleep(FOLLOW_AFTER + SETTLE);
            assert_eq!(fx.engine.calls().len(), 2, "a screen that stays as it was asks nothing new");
            nothing_typed(&holder);
            let stuck = fx.waits_for_you(id);
            assert_eq!(stuck.problem.as_deref(), Some("Octo's answer did not take; answer it yourself"));
            assert_eq!(stuck.question, "Step 2: continue?", "an answer that changed nothing comes back to the user");
            assert!(stuck.suggestion.is_some(), "with what Octo had answered");
        }

        #[test]
        fn after_five_follow_ups_in_one_pause_the_next_question_is_left_to_the_user() {
            let fx = fixture("follow-limit", PRESS_ONE);
            fx.enable(Mode::Moderate);
            let (id, holder) = fx.agent_showing("alpha", AgentKind::Codex, 100, Arc::new(asks));
            fx.hub.orchestrate(true);
            let answers = usize::from(MAX_ANSWERS_IN_A_ROW);
            for answer in 0..answers {
                assert_eq!(typed_into(&holder, 1), sent(&[b"1"]), "answer {answer}");
            }
            let left = fx.waits_for_you(id);
            assert_eq!(left.question, format!("Step {answers}: continue?"));
            assert_eq!(
                left.problem.as_deref(),
                Some("Octo already answered 5 questions in a row here; this one is yours")
            );
            assert_eq!(left.suggestion, None);
            assert_eq!(fx.engine.calls().len(), answers, "the question beyond the limit is not judged");
            nothing_typed(&holder);
        }

        #[test]
        fn a_second_permission_asked_by_claude_in_the_same_pause_is_seen_through_its_hook() {
            let fx = fixture("follow-ask", PRESS_ONE);
            fx.enable(Mode::Bypass);
            let (id, holder) = fx.agent("alpha", AgentKind::Claude, 100);
            let asked = |detail: &str, at_ms| {
                let ask = Ask { tool: "Bash".into(), detail: detail.into(), at_ms };
                hooks::write_ask(fx.store.home(), id, &ask).unwrap();
            };
            asked("npm test", 100);
            fx.hub.orchestrate(true);
            assert_eq!(typed_into(&holder, 1), sent(&[b"\r"]));
            asked("npm run build", now_ms() + 1_000);
            assert_eq!(typed_into(&holder, 1), sent(&[b"\r"]), "the screen is the same but the hook asked again");
            let logged = until("both are logged", || Some(fx.view().handled).filter(|handled| handled.len() == 2));
            assert_eq!(logged[0].summary, "Allowed Bash: npm run build");
            assert_eq!(logged[1].summary, "Allowed Bash: npm test");
            assert!(logged.iter().all(|entry| entry.by == By::Rule));
            assert!(fx.engine.calls().is_empty());
        }

        #[test]
        fn a_judgement_whose_agent_went_on_is_stopped_and_its_second_engine_is_never_asked() {
            let fx = fixture_with("stop", Duration::from_millis(500), Box::new(|_, _| Err("claude: boom".to_string())));
            fx.enable(Mode::Moderate);
            fx.set(SettingsChange { secondary: Some(Some(AgentKind::Codex)), ..SettingsChange::default() });
            let (alpha, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            until("the engine is asked", || (fx.engine.calls().len() == 1).then_some(()));
            let judged = fx.decision_of(alpha).unwrap().id;
            fx.status(alpha, AgentState::Working, 200);
            fx.hub.orchestrate(true);
            until("its engine is stopped", || (*lock(&fx.engine.stopped) == [judged]).then_some(()));
            thread::sleep(Duration::from_millis(800));
            assert_eq!(fx.engine.calls().len(), 1, "the secondary engine is not asked about a question that is gone");
            assert_eq!(fx.view().problem, None, "a judgement that was stopped is not a failure to show");
            assert!(fx.view().handled.is_empty());
            nothing_typed(&holder);

            let (beta, _beta_holder) = fx.agent("beta", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            until("the engine is asked again", || (fx.engine.calls().len() == 2).then_some(()));
            let judged_beta = fx.decision_of(beta).unwrap().id;
            fx.set(SettingsChange { enabled: Some(false), ..SettingsChange::default() });
            until("turning Octo off stops what it was judging", || {
                lock(&fx.engine.stopped).contains(&judged_beta).then_some(())
            });
        }

        #[test]
        fn nothing_new_starts_once_octo_is_shutting_down() {
            let failing: Script = Box::new(|_, _| Err("claude: boom".to_string()));
            let fx = fixture_with("closing", Duration::from_millis(400), failing);
            fx.enable(Mode::Moderate);
            fx.set(SettingsChange { secondary: Some(Some(AgentKind::Codex)), ..SettingsChange::default() });
            for index in 0..4 {
                fx.paused(&format!("agent-{index}"), AgentKind::Codex, 100);
            }
            fx.hub.orchestrate(true);
            until("two engines run", || (fx.engine.calls().len() == MAX_ENGINE_PROCESSES).then_some(()));
            assert!(!fx.octo().is_stopping());
            fx.octo().shut_down();
            assert!(fx.octo().is_stopping());
            thread::sleep(Duration::from_millis(1_200));
            assert_eq!(
                fx.engine.calls().len(),
                MAX_ENGINE_PROCESSES,
                "neither the queued judgements nor the secondary engine start while the app closes"
            );
        }

        #[test]
        fn an_answer_or_a_dismissal_given_while_octo_still_judges_stops_its_engine_and_counts_once() {
            let fx = fixture_with("claim", Duration::from_millis(600), Box::new(|_, _| Ok(PRESS_ONE.to_string())));
            fx.enable(Mode::Moderate);
            let (alpha, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            let (beta, beta_holder) = fx.agent("beta", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            let read = |agent: AgentId| {
                until("the question is read", || fx.decision_of(agent).filter(|decision| !decision.question.is_empty()))
            };
            let (first, second) = (read(alpha), read(beta));
            until("both are being judged", || (fx.engine.calls().len() == 2).then_some(()));
            let typed = || UserAnswer {
                decision: first.id,
                how: How::Text,
                text: Some("mine".into()),
                keys: None,
                remember: Some("answer these yourself".into()),
            };
            fx.octo().answer_from_user(&fx.hub, typed(), &fx.reply);
            fx.octo().answer_from_user(&fx.hub, typed(), &fx.reply);
            let dismiss = UserAnswer { decision: second.id, how: How::Dismiss, text: None, keys: None, remember: None };
            fx.octo().answer_from_user(&fx.hub, dismiss, &fx.reply);
            assert_eq!(fx.notice(), (Tone::Warn, "that decision is no longer pending".to_string()), "the second tap");
            assert_eq!(typed_into(&holder, 2), sent(&[b"mine", b"\r"]));
            until("both engines are stopped", || {
                let stopped = lock(&fx.engine.stopped);
                (stopped.contains(&first.id) && stopped.contains(&second.id)).then_some(())
            });
            thread::sleep(Duration::from_millis(900));
            nothing_typed(&holder);
            nothing_typed(&beta_holder);
            assert_eq!(
                instructions::read(fx.store.home(), Some(&first.project)).unwrap(),
                "- answer these yourself\n",
                "a rule sent twice by a double tap is remembered once"
            );
            let logged = fx.view().handled;
            assert_eq!(logged.len(), 1, "the verdicts that came too late are neither applied nor logged");
            assert_eq!((logged[0].by, logged[0].summary.as_str()), (By::You, "Typed: mine"));
        }

        #[test]
        fn shutting_down_stops_every_engine_and_saves_what_was_not_yet_on_disk() {
            let fx = fixture("shutdown", ESCALATE);
            fx.enable(Mode::Moderate);
            let (id, _holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            fx.waits_for_you(id);
            assert_eq!(fx.engine.stopped_all.load(Ordering::SeqCst), 0);
            fx.octo().shut_down();
            assert_eq!(fx.engine.stopped_all.load(Ordering::SeqCst), 1);
            let journal = std::fs::read_to_string(fx.store.home().orchestrator_dir().join("activity.jsonl")).unwrap();
            assert_eq!(journal.lines().count(), 1, "nothing logged is lost when the app closes");
        }

        #[test]
        fn a_judgement_that_cannot_start_is_left_to_the_user_with_the_reason() {
            let fx = fixture_with("give-up", Duration::from_millis(500), Box::new(|_, _| Ok(PRESS_ONE.to_string())));
            fx.enable(Mode::Moderate);
            let (id, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(true);
            until("the question is read", || fx.decision_of(id).filter(|decision| !decision.question.is_empty()));
            let judged = fx.decision_of(id).unwrap().id;
            fx.octo().give_up(&fx.hub, Some(judged));
            let left = fx.waits_for_you(id);
            assert_eq!(left.problem.as_deref(), Some("Octo could not start judging this; answer it yourself"));
            thread::sleep(Duration::from_millis(800));
            nothing_typed(&holder);
            assert_eq!(fx.waits_for_you(id).suggestion, None, "an answer that comes for it later is not used");
        }

        struct Phone(Vec<BridgeMsg>);

        impl Outbox for Phone {
            fn deliver(&mut self, plain: &[u8]) -> std::io::Result<()> {
                self.0.push(serde_json::from_slice(plain).map_err(std::io::Error::other)?);
                Ok(())
            }
        }

        fn bridge_at(store: &Store) -> (Arc<Hub>, Peer, Phone) {
            let launcher = Launcher::new(store.clone(), Config::default(), &std::env::current_exe().unwrap()).unwrap();
            let hub = Hub::new(launcher, "bridge".into());
            let peer = Peer::join(&hub);
            (hub, peer, Phone(Vec::new()))
        }

        fn phone_sees<T>(peer: &mut Peer, phone: &mut Phone, wanted: impl Fn(&BridgeMsg) -> Option<T>) -> T {
            let deadline = Instant::now() + WAIT;
            while Instant::now() < deadline {
                peer.wait(phone).unwrap();
                if let Some(found) = phone.0.iter().find_map(&wanted) {
                    return found;
                }
            }
            panic!("the phone never saw it; it saw {} messages", phone.0.len());
        }

        fn view_where(
            peer: &mut Peer,
            phone: &mut Phone,
            check: impl Fn(&OrchestratorView) -> bool,
        ) -> OrchestratorView {
            phone_sees(peer, phone, |message| match message {
                BridgeMsg::Orchestrator(view) if check(view) => Some(view.clone()),
                _ => None,
            })
        }

        fn turn_on() -> PhoneMsg {
            PhoneMsg::OrchestratorSet(SettingsChange {
                enabled: Some(true),
                mode: Some(Mode::Moderate),
                primary: Some(Some(AgentKind::Claude)),
                ..SettingsChange::default()
            })
        }

        #[test]
        fn a_phone_turns_octo_on_answers_and_talks_through_the_bridge() {
            let fx = fixture_with(
                "relay",
                Duration::ZERO,
                Box::new(|_, prompt| {
                    let answer = if prompt.contains(CHAT_PROMPT) { "All quiet." } else { ESCALATE };
                    Ok(answer.to_string())
                }),
            );
            let home = fx.store.home().clone();
            let (bridge, mut peer, mut phone) = bridge_at(&fx.store);
            assert!(bridge.orchestrator().is_none(), "the hub of the bridge never orchestrates");
            fx.octo().set_relaying(true);
            fx.hub.orchestrate(true);
            view_where(&mut peer, &mut phone, |view| view.online && !view.enabled);

            peer.handle(turn_on(), &mut phone).unwrap();
            assert!(relay::has_commands(&home), "the command waits in the inbox of the desktop");
            fx.hub.orchestrate(false);
            thread::sleep(SETTLE);
            assert!(!fx.octo().is_enabled(), "while Octo is off its inbox is only looked at when the hub rescans");
            fx.hub.orchestrate(true);
            until("the phone turned Octo on", || fx.octo().is_enabled().then_some(()));
            let on = view_where(&mut peer, &mut phone, |view| view.online && view.enabled);
            assert_eq!((on.mode, on.primary), (Mode::Moderate, Some(AgentKind::Claude)));

            let (id, holder) = fx.agent("alpha", AgentKind::Codex, 100);
            fx.hub.orchestrate(false);
            let pending = view_where(&mut peer, &mut phone, |view| {
                view.decisions
                    .iter()
                    .any(|decision| decision.agent == id && decision.status == DecisionStatus::NeedsYou)
            });
            let decision = pending.decisions[0].clone();
            assert_eq!(decision.question, QUESTION);
            let allow = UserAnswer { decision: decision.id, how: How::Allow, text: None, keys: None, remember: None };
            peer.handle(PhoneMsg::OrchestratorAnswer(allow), &mut phone).unwrap();
            nothing_typed(&holder);
            fx.hub.orchestrate(false);
            assert_eq!(typed_into(&holder, 1), sent(&[b"\r"]), "the answer of the phone reaches the agent");
            let answered =
                view_where(&mut peer, &mut phone, |view| view.handled.iter().any(|entry| entry.by == By::You));
            assert_eq!(answered.handled[0].summary, "Allowed it");
            assert!(answered.decisions.is_empty());

            peer.handle(PhoneMsg::OrchestratorChat { text: "anything running?".into() }, &mut phone).unwrap();
            fx.hub.orchestrate(false);
            let talked = view_where(&mut peer, &mut phone, |view| view.chat.len() == 2 && !view.thinking);
            assert_eq!(
                (talked.chat[0].text.as_str(), talked.chat[1].text.as_str()),
                ("anything running?", "All quiet.")
            );
            peer.handle(PhoneMsg::OrchestratorClearChat, &mut phone).unwrap();
            fx.hub.orchestrate(false);
            until("the phone cleared the conversation", || fx.view().chat.is_empty().then_some(()));
            assert!(!relay::has_commands(&home), "every command was read once and deleted");

            peer.handle(PhoneMsg::OrchestratorInstructions { project: None }, &mut phone).unwrap();
            let rules = phone_sees(&mut peer, &mut phone, |message| match message {
                BridgeMsg::OrchestratorInstructions { project: None, text, .. } => Some(text.clone()),
                _ => None,
            });
            assert_eq!(rules, instructions::DEFAULT, "the bridge reads the file itself");
            let project = Some(decision.project.clone());
            let save =
                PhoneMsg::OrchestratorSaveInstructions { project: project.clone(), text: "- from the phone\n".into() };
            peer.handle(save, &mut phone).unwrap();
            phone_sees(&mut peer, &mut phone, |message| match message {
                BridgeMsg::OrchestratorInstructions { project: Some(_), text, .. } => Some(text.clone()),
                _ => None,
            });
            assert_eq!(
                instructions::read(&home, project.as_deref()).unwrap(),
                "- from the phone\n",
                "and writes the file the desktop reads before each judgement"
            );
            until("the activity is on disk", || {
                peer.handle(PhoneMsg::OrchestratorActivity { before: None }, &mut phone).unwrap();
                thread::sleep(Duration::from_millis(50));
                peer.pump(&mut phone).unwrap();
                phone.0.iter().find_map(|message| match message {
                    BridgeMsg::OrchestratorActivity { items, .. } if items.len() == 2 => Some(()),
                    _ => None,
                })
            });
        }

        #[test]
        fn a_phone_is_told_when_octo_is_not_open_on_the_computer() {
            let dir = std::env::temp_dir().join(format!("orkai-hub-offline-{}-{}", std::process::id(), now_ms()));
            let store = Store::open(Home::at(dir.join("home"))).unwrap();
            let home = store.home().clone();
            let last =
                OrchestratorView { online: true, enabled: true, mode: Mode::Bypass, ..OrchestratorView::default() };
            relay::write_view(&home, &last).unwrap();
            let (_bridge, mut peer, mut phone) = bridge_at(&store);
            let shown = view_where(&mut peer, &mut phone, |_| true);
            assert!(!shown.online, "nobody holds the lock of the desktop");
            assert_eq!((shown.enabled, shown.mode), (true, Mode::Bypass), "the last view it wrote is still shown");

            peer.handle(PhoneMsg::Orchestrator, &mut phone).unwrap();
            let allow = UserAnswer { decision: 1, how: How::Allow, text: None, keys: None, remember: None };
            let commands = [
                turn_on(),
                PhoneMsg::OrchestratorAnswer(allow),
                PhoneMsg::OrchestratorChat { text: "hi".into() },
                PhoneMsg::OrchestratorClearChat,
            ];
            let refused = commands.len();
            for command in commands {
                peer.handle(command, &mut phone).unwrap();
            }
            let warned = |message: &BridgeMsg| matches!(message, BridgeMsg::Notice { text, tone: Tone::Warn } if text == relay::OFFLINE);
            until("every command is refused", || {
                peer.wait(&mut phone).unwrap();
                (phone.0.iter().filter(|message| warned(message)).count() == refused).then_some(())
            });
            assert!(!relay::has_commands(&home), "nothing is queued for a desktop that is not there");

            peer.handle(PhoneMsg::OrchestratorInstructions { project: None }, &mut phone).unwrap();
            let rules = phone_sees(&mut peer, &mut phone, |message| match message {
                BridgeMsg::OrchestratorInstructions { text, .. } => Some(text.clone()),
                _ => None,
            });
            assert_eq!(rules, instructions::DEFAULT, "the files can be read without the desktop");
            peer.handle(PhoneMsg::OrchestratorActivity { before: None }, &mut phone).unwrap();
            let page = phone_sees(&mut peer, &mut phone, |message| match message {
                BridgeMsg::OrchestratorActivity { items, next } => Some((items.len(), *next)),
                _ => None,
            });
            assert_eq!(page, (0, None));

            let held = relay::hold(&home).unwrap();
            view_where(&mut peer, &mut phone, |view| view.online);
            drop(held);
            until("the desktop closing is noticed", || {
                peer.wait(&mut phone).unwrap();
                let mut views = phone.0.iter().filter_map(|message| match message {
                    BridgeMsg::Orchestrator(view) => Some(view.online),
                    _ => None,
                });
                (views.next_back() == Some(false)).then_some(())
            });
            drop(peer);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn the_desktop_reads_its_inbox_only_while_the_phone_bridge_is_on_and_drops_stale_commands() {
            let fx = fixture("inbox", ESCALATE);
            let home = fx.store.home().clone();
            relay::send_command(&home, &turn_on(), now_ms()).unwrap();
            for _ in 0..3 {
                fx.hub.orchestrate(true);
            }
            thread::sleep(SETTLE);
            assert!(!fx.octo().is_awake() && !fx.octo().is_enabled());
            assert!(relay::has_commands(&home), "with the bridge off nothing is read");
            assert!(!relay::view_file(&home).exists(), "and no view is written for a phone that cannot come");
            assert_eq!(relay::take_commands(&home, now_ms()).len(), 1);

            fx.octo().set_relaying(true);
            assert!(fx.octo().is_awake(), "a phone bridge that is on keeps the relay awake even with Octo off");
            relay::send_command(&home, &turn_on(), now_ms() - relay::COMMAND_TTL_MS - 1_000).unwrap();
            fx.hub.orchestrate(true);
            until("the stale command is taken", || (!relay::has_commands(&home)).then_some(()));
            thread::sleep(SETTLE);
            assert!(!fx.octo().is_enabled(), "a command that waited more than a minute is dropped");
            let written = until("the phone is told Octo is off", || std::fs::metadata(relay::view_file(&home)).ok());
            let told: OrchestratorView =
                serde_json::from_slice(&std::fs::read(relay::view_file(&home)).unwrap()).unwrap();
            assert!(told.online && !told.enabled);
            for _ in 0..3 {
                fx.hub.orchestrate(true);
            }
            thread::sleep(SETTLE);
            let after = std::fs::metadata(relay::view_file(&home)).unwrap();
            assert_eq!(after.modified().unwrap(), written.modified().unwrap(), "and it is told once, not every tick");

            relay::send_command(&home, &turn_on(), now_ms()).unwrap();
            fx.hub.orchestrate(true);
            until("a fresh command turns Octo on", || fx.octo().is_enabled().then_some(()));
            until("and the new view is written", || {
                let view: Option<OrchestratorView> =
                    std::fs::read(relay::view_file(&home)).ok().and_then(|data| serde_json::from_slice(&data).ok());
                view.filter(|view| view.enabled)
            });
            fx.octo().set_relaying(false);
            assert!(fx.octo().is_awake(), "Octo itself is on now");
            fx.set(SettingsChange { enabled: Some(false), ..SettingsChange::default() });
            assert!(!fx.octo().is_awake(), "off and without a phone bridge every tick is skipped again");
        }

        #[test]
        fn the_hub_learns_from_its_scan_whether_the_phone_bridge_is_on() {
            let fx = fixture("relaying", ESCALATE);
            let mut ui = fx.store.load_ui();
            ui.pwa = true;
            fx.store.save_ui(&ui).unwrap();
            assert!(!fx.octo().is_relaying());
            fx.hub.rescan();
            assert!(fx.octo().is_relaying() && fx.octo().is_awake());
            ui.pwa = false;
            fx.store.save_ui(&ui).unwrap();
            fx.hub.rescan();
            assert!(!fx.octo().is_relaying() && !fx.octo().is_awake());
        }
    }
}
