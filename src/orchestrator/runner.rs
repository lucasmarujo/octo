use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::chat::{self, LastTurn};
use crate::cli::{DEFAULT_SIZE, SUBMIT_DELAY};
use crate::client;
use crate::config::Config;
use crate::holder::detect::DONE_GRACE_MS;
use crate::holder::term::encode_typed_text;
use crate::hooks;
use crate::launch::LaunchMode;
use crate::model::{AgentId, AgentKind, AgentRecord, AgentState};
use crate::models;
use crate::orchestrator::engine::{Call, Engine};
use crate::orchestrator::logs::{self, Activity, Handled, Journal, Talk};
use crate::orchestrator::prompt::{
    self, Case, MAX_CHAT_HISTORY, MAX_LAST_MESSAGE_CHARS, MAX_TASK_CHARS, Rules, TurnEnd,
};
use crate::orchestrator::question::{allowed_by_rule, context_rows, fresh_ask, question, tool_of};
use crate::orchestrator::relay::{self, Presence};
use crate::orchestrator::settings::{Settings, SettingsChange};
use crate::orchestrator::verdict::{
    self, ChatAction, ChatAnswer, Judged, Opinion, Review, Ruling, Verdict, encode_keys, usable_input,
};
use crate::orchestrator::{
    By, ChatEntry, ChatRole, Decision, DecisionStatus, ENGINES, EngineView, How, Input, MAX_DECISIONS,
    MAX_ENGINE_PROCESSES, MAX_QUESTION_CHARS, Mode, OrchestratorView, Outcome, Sight, Sighting, Suggestion, UserAnswer,
    cut, instructions, one_line,
};
use crate::paths::Home;
use crate::protocol::ClientMsg;
use crate::pwa::actions::{self, notice, spawn};
use crate::pwa::hub::{Event, Hub};
use crate::pwa::protocol::{AgentView, BridgeMsg, PhoneMsg, StartRequest, Tone};
use crate::pwa::server::SUBMIT;
use crate::store::Store;
use crate::termfmt;
use crate::tui::keys;
use crate::util::{local_offset_ms, lock, now_ms};

pub const MAX_CHAT_CHARS: usize = 4_000;
pub const MAX_ANSWERS_IN_A_ROW: u8 = 5;
pub const MAX_REPLIES_IN_A_ROW: u8 = 20;
pub const MAX_CLIENT_JOBS: usize = 4;
pub const SETTLE: Duration = Duration::from_millis(DONE_GRACE_MS);
const CHAT_TICKET: u64 = 0;
const MAX_LINE_CHARS: usize = 80;
pub const MAX_SENT_CHARS: usize = 280;
const CUT_MARK: &str = "…";
const KEY_GAP: Duration = Duration::from_millis(80);
const ENTER: &str = "Enter";
const NO_ENGINE: &str = "choose an engine for Octo in its settings";
const ANSWERED_IN_AGENT: &str = "Answered in the agent";
const LEFT_FOR_YOU: &str = "Left for you";
const ANSWERED: &str = "Answered";
const ALLOWED: &str = "Allowed it";
const FINISHED: &str = "Finished its turn";
const REPLIED: &str = "Replied to it";
const WRONG_ACTION: &str = "Skipped an action Octo wrote wrongly";
const DID_NOT_TAKE: &str = "Octo's answer did not take; answer it yourself";
const BUSY: &str = "Octo is busy; try again";
const NOT_PENDING: &str = "that decision is no longer pending";
const NO_THREAD: &str = "Octo could not start judging this; answer it yourself";
const STOPPED: &str = "the judgement was stopped";
const READER_THREAD: &str = "orkai-octo-read";
const ENGINE_THREAD: &str = "orkai-octo-engine";
const ANSWER_THREAD: &str = "orkai-octo-answer";
const RULES_THREAD: &str = "orkai-octo-rules";
const ACTIVITY_THREAD: &str = "orkai-octo-activity";
const CHORES_THREAD: &str = "orkai-octo-chores";
const INBOX_THREAD: &str = "orkai-octo-inbox";
const FOLLOW_THREAD: &str = "orkai-octo-follow";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Reading,
    Judging,
    NeedsYou,
    Answering,
    Settled,
}

struct Open {
    decision: Decision,
    stage: Stage,
    task: String,
    app_cursor: bool,
    bracketed: bool,
    asked_from: u64,
    follow_ups: u8,
    sight: Sight,
    last_turn: LastTurn,
}

#[derive(Clone, Copy)]
struct Reviewed {
    since: u64,
    replies: u8,
}

impl Open {
    fn wait_for_user(&mut self, problem: Option<String>) {
        self.stage = Stage::NeedsYou;
        self.decision.status = DecisionStatus::NeedsYou;
        self.decision.problem = problem;
    }

    fn is_taken(&self) -> bool {
        matches!(self.stage, Stage::Answering | Stage::Settled)
    }

    fn handled(&self, credit: Credit, mode: Mode) -> Handled {
        Handled {
            agent: self.decision.agent,
            name: self.decision.name.clone(),
            kind: self.decision.kind,
            project: self.decision.project.clone(),
            question: self.decision.question.clone(),
            summary: credit.summary,
            by: credit.by,
            outcome: credit.outcome,
            mode,
        }
    }
}

struct Credit {
    by: By,
    outcome: Outcome,
    summary: String,
}

enum Job {
    Judge(u64),
    Chat { text: String, reply: Sender<Event> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answered {
    Sent,
    Stale,
    Gone,
    Failed(String),
}

struct Screen {
    rows: Vec<String>,
    app_cursor: bool,
    bracketed: bool,
}

struct State {
    settings: Settings,
    loaded: bool,
    decisions: BTreeMap<AgentId, Open>,
    next_decision: u64,
    activity: Activity,
    talk: Talk,
    thinking: bool,
    problem: Option<String>,
    engines: Vec<EngineView>,
    queue: VecDeque<Job>,
    running: usize,
    reading: bool,
    dirty: bool,
    view_unsent: bool,
    stops: Vec<u64>,
    tidying: bool,
    watching_since: u64,
    reviewed: BTreeMap<AgentId, Reviewed>,
}

impl State {
    fn load(&mut self, now: u64) {
        self.activity.load(now, local_offset_ms(now));
        self.talk.load();
        self.loaded = true;
    }

    fn record(&mut self, handled: Handled) {
        let now = now_ms();
        self.activity.record(handled, now, local_offset_ms(now));
        self.dirty = true;
    }

    fn open(&mut self, decision: Decision, task: String, follow_ups: u8, asked_from: u64, sight: Sight) {
        let open = Open {
            decision,
            stage: Stage::Reading,
            task,
            app_cursor: false,
            bracketed: false,
            asked_from,
            follow_ups,
            sight,
            last_turn: LastTurn::default(),
        };
        self.decisions.insert(open.decision.agent, open);
        self.dirty = true;
    }

    fn is_unreviewed(&self, agent: AgentId, since: u64) -> bool {
        since >= self.watching_since && self.reviewed.get(&agent).is_none_or(|reviewed| reviewed.since != since)
    }

    fn replies_in_a_row(&self, agent: AgentId) -> u8 {
        self.reviewed.get(&agent).map_or(0, |reviewed| reviewed.replies)
    }

    fn count_reply(&mut self, agent: AgentId, replies: impl Fn(u8) -> u8) {
        if let Some(reviewed) = self.reviewed.get_mut(&agent) {
            reviewed.replies = replies(reviewed.replies);
        }
    }

    fn tell(&mut self, text: String, action: String) {
        self.talk.say(ChatEntry { at: now_ms(), role: ChatRole::Octo, text, actions: vec![action], failed: false });
        self.dirty = true;
    }

    fn stop_judging(&mut self, open: &Open) {
        if open.stage == Stage::Judging {
            self.stops.push(open.decision.id);
        }
    }

    fn pending(&self) -> Vec<Decision> {
        let mut decisions: Vec<Decision> = self
            .decisions
            .values()
            .filter(|open| open.stage != Stage::Settled)
            .map(|open| open.decision.clone())
            .collect();
        decisions.sort_by_key(|decision| decision.id);
        decisions
    }

    fn view(&self, now: u64) -> OrchestratorView {
        let (handled_today, handled_by_project) = self.activity.today(now, local_offset_ms(now));
        OrchestratorView {
            online: true,
            enabled: self.settings.enabled,
            mode: self.settings.mode,
            projects: self.settings.projects.clone(),
            primary: self.settings.primary,
            secondary: self.settings.secondary,
            primary_model: self.settings.primary_model.clone(),
            secondary_model: self.settings.secondary_model.clone(),
            engines: self.engines.clone(),
            decisions: self.pending(),
            handled: self.activity.handled(),
            handled_today,
            handled_by_project,
            chat: self.talk.entries(),
            thinking: self.thinking,
            problem: self.problem.clone(),
        }
    }
}

pub fn too_many_questions() -> String {
    format!("Octo already answered {MAX_ANSWERS_IN_A_ROW} questions in a row here; this one is yours")
}

pub fn too_many_replies() -> String {
    format!("Octo already replied {MAX_REPLIES_IN_A_ROW} times in a row here without you; this one is yours")
}

pub fn turn_note(agent: AgentId, name: &str, report: &str, fallback: &str) -> String {
    let said = report.trim();
    format!("{name} (#{agent}): {}", if said.is_empty() { fallback } else { said })
}

pub fn waiting_for(said: &str, rows: &[String]) -> String {
    let last = said.lines().rev().map(str::trim).find(|line| !line.is_empty());
    last.map_or_else(|| question(None, rows), |line| cut(line, MAX_QUESTION_CHARS))
}

type Typed = Option<(Input, String)>;

pub fn typed_by(
    how: How,
    text: Option<String>,
    keys: Option<Vec<String>>,
    suggestion: Option<&Suggestion>,
) -> Result<Typed, &'static str> {
    let described = |input: Input| {
        let said = describe(&input);
        Some((input, said))
    };
    match how {
        How::Dismiss => Ok(None),
        How::Allow => Ok(Some((Input::Keys(vec![ENTER.to_string()]), ALLOWED.to_string()))),
        How::Text => usable_input(None, text).map(described).ok_or("write what to answer"),
        How::Keys => usable_input(keys, None).map(described).ok_or("those keys cannot be pressed"),
        How::Suggestion => match suggestion.and_then(|suggested| Some((suggested.input()?, suggested.summary.clone())))
        {
            Some((input, summary)) if summary.is_empty() => Ok(described(input)),
            Some(suggested) => Ok(Some(suggested)),
            None => Err("Octo has no suggestion for this one"),
        },
    }
}

#[derive(Clone, Default)]
pub struct Jobs {
    running: Arc<AtomicUsize>,
}

struct JobSlot(Arc<AtomicUsize>);

impl Drop for JobSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Jobs {
    pub fn run(&self, name: &str, reply: &Sender<Event>, work: impl FnOnce() + Send + 'static) -> bool {
        let slot = JobSlot(Arc::clone(&self.running));
        let free = self.running.fetch_add(1, Ordering::SeqCst) < MAX_CLIENT_JOBS;
        let started = free
            && thread::Builder::new()
                .name(name.to_string())
                .spawn(move || {
                    let _slot = slot;
                    work();
                })
                .is_ok();
        if !started {
            notice(reply, Tone::Warn, BUSY);
        }
        started
    }
}

fn find(decisions: &mut BTreeMap<AgentId, Open>, id: u64) -> Option<&mut Open> {
    decisions.values_mut().find(|open| open.decision.id == id)
}

pub fn engine_views(installed: &[AgentKind]) -> Vec<EngineView> {
    ENGINES.iter().map(|kind| EngineView { kind: *kind, installed: installed.contains(kind) }).collect()
}

fn session_names(store: &Store) -> Vec<String> {
    store.load_sessions().unwrap_or_default().into_iter().map(|session| session.name).collect()
}

fn start_models(installed: &[AgentKind]) -> Vec<(AgentKind, Vec<String>)> {
    installed
        .iter()
        .map(|kind| (*kind, models::available(*kind).into_iter().map(|model| model.id).collect::<Vec<_>>()))
        .filter(|(_, ids)| !ids.is_empty())
        .collect()
}

pub fn session_for(
    requested: Option<&str>,
    cwd: &Path,
    sessions: &[String],
    agents: &[AgentView],
    remembered: &str,
) -> Option<String> {
    let beside = agents
        .iter()
        .filter(|agent| actions::resolve_dir(&agent.cwd) == cwd)
        .max_by_key(|agent| agent.id)
        .map(|agent| agent.session.as_str());
    requested
        .into_iter()
        .chain(beside)
        .chain(Some(remembered))
        .chain(sessions.first().map(String::as_str))
        .find(|name| sessions.iter().any(|session| session == name))
        .map(str::to_string)
}

fn started(record: &AgentRecord) -> String {
    let model = record.model.as_deref().map_or_else(String::new, |model| format!(" on {model}"));
    format!(
        "Started {} {} (#{}){model} in {}, session {}",
        record.kind.label(),
        record.name,
        record.id,
        record.cwd.display(),
        record.session
    )
}

pub fn sight_of(seen: &[Sighting], agent: AgentId) -> Option<Sight> {
    seen.binary_search_by_key(&agent, |(id, _)| *id).ok().map(|at| seen[at].1)
}

pub fn chat_answer(said: &str) -> Option<ChatAnswer> {
    let read = verdict::first_object::<ChatAnswer>(said)
        .filter(|answer| !answer.said().is_empty() || !answer.reads().is_empty() || !answer.actions().is_empty());
    let prose = || Some(said.trim()).filter(|text| !text.is_empty());
    read.or_else(|| prose().map(|text| ChatAnswer { reply: text.to_string(), read: None, actions: None }))
}

pub fn describe(input: &Input) -> String {
    match input {
        Input::Keys(keys) => format!("Pressed {}", keys.join(" ")),
        Input::Text(text) => format!("Typed: {}", one_line(text, MAX_LINE_CHARS)),
    }
}

fn read_screen(store: &Store, agent: AgentId) -> Option<Screen> {
    let runtime = store.read_runtime(agent).ok().flatten()?;
    let snapshot = client::peek(&runtime).ok()?;
    let mut parser = vt100::Parser::new(snapshot.size.rows, snapshot.size.cols, 0);
    parser.process(&snapshot.data);
    let screen = parser.screen();
    Some(Screen {
        rows: context_rows(&termfmt::screen_tail(screen, usize::from(snapshot.size.rows))),
        app_cursor: screen.application_cursor(),
        bracketed: screen.bracketed_paste(),
    })
}

fn last_turn_of(hub: &Hub, agent: AgentId) -> LastTurn {
    hub.record(agent)
        .and_then(|record| actions::claude_transcript_of(hub, &record))
        .map(|(_, path)| chat::last_turn(&path, MAX_LAST_MESSAGE_CHARS))
        .unwrap_or_default()
}

pub fn sent_line(name: &str, text: &str) -> String {
    let shown = one_line(text, MAX_SENT_CHARS);
    let cut_short = one_line(text, MAX_SENT_CHARS + 1).chars().count() > MAX_SENT_CHARS;
    format!("Sent to {name}: {shown}{}", if cut_short { CUT_MARK } else { "" })
}

fn is_still(hub: &Hub, agent: AgentId, sight: Sight) -> bool {
    hub.sight(agent) == Some(sight)
}

fn type_input(hub: &Hub, agent: AgentId, input: &Input, app_cursor: bool, bracketed: bool) -> io::Result<()> {
    match input {
        Input::Keys(names) => {
            let keys = encode_keys(names, app_cursor)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "those keys cannot be pressed"))?;
            for (at, key) in keys.into_iter().enumerate() {
                if at > 0 {
                    thread::sleep(KEY_GAP);
                }
                hub.send(agent, &ClientMsg::Input(key))?;
            }
            Ok(())
        }
        Input::Text(text) => {
            let bytes = if bracketed { keys::encode_paste(text.trim(), true) } else { encode_typed_text(text, false) };
            if !bytes.is_empty() {
                hub.send(agent, &ClientMsg::Input(bytes))?;
                thread::sleep(SUBMIT_DELAY);
            }
            hub.send(agent, &ClientMsg::Input(SUBMIT.to_vec()))
        }
    }
}

struct Target {
    agent: AgentId,
    sight: Sight,
    app_cursor: bool,
    bracketed: bool,
}

fn deliver(hub: &Hub, target: &Target, input: &Input) -> Answered {
    let Target { agent, sight, app_cursor, bracketed } = *target;
    if !is_still(hub, agent, sight) {
        return Answered::Stale;
    }
    match type_input(hub, agent, input, app_cursor, bracketed) {
        Ok(()) => Answered::Sent,
        Err(err) => Answered::Failed(format!("the answer did not reach the agent: {err}")),
    }
}

struct Slot {
    octo: Arc<Orchestrator>,
    hub: Arc<Hub>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.octo.state);
            state.running = state.running.saturating_sub(1);
        }
        self.octo.pump(&self.hub);
    }
}

pub struct Orchestrator {
    home: Home,
    engine: Arc<dyn Engine>,
    enabled: AtomicBool,
    relaying: AtomicBool,
    awake: AtomicBool,
    sorting: AtomicBool,
    stopping: AtomicBool,
    presence: Mutex<Option<Presence>>,
    disk: Mutex<()>,
    state: Mutex<State>,
}

impl Drop for Orchestrator {
    fn drop(&mut self) {
        self.shut_down();
    }
}

impl Orchestrator {
    pub fn open(home: &Home, engine: Arc<dyn Engine>) -> Arc<Self> {
        let settings = Settings::load(home);
        let enabled = settings.enabled;
        let mut state = State {
            watching_since: now_ms(),
            reviewed: BTreeMap::new(),
            settings,
            loaded: false,
            decisions: BTreeMap::new(),
            next_decision: 1,
            activity: Activity::new(Journal::activity(home)),
            talk: Talk::new(Journal::chat(home)),
            thinking: false,
            problem: None,
            engines: Vec::new(),
            queue: VecDeque::new(),
            running: 0,
            reading: false,
            dirty: false,
            view_unsent: false,
            stops: Vec::new(),
            tidying: false,
        };
        if enabled {
            state.load(now_ms());
        }
        Arc::new(Self {
            home: home.clone(),
            engine,
            enabled: AtomicBool::new(enabled),
            relaying: AtomicBool::new(false),
            awake: AtomicBool::new(enabled),
            sorting: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            presence: Mutex::new(relay::hold(home).ok()),
            disk: Mutex::new(()),
            state: Mutex::new(state),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn is_awake(&self) -> bool {
        self.awake.load(Ordering::Relaxed)
    }

    pub fn is_relaying(&self) -> bool {
        self.relaying.load(Ordering::Relaxed)
    }

    fn wake(&self) {
        self.awake.store(self.is_enabled() || self.is_relaying(), Ordering::Relaxed);
    }

    pub fn set_relaying(&self, relaying: bool) {
        if self.relaying.swap(relaying, Ordering::Relaxed) == relaying {
            return;
        }
        self.wake();
        lock(&self.state).view_unsent = relaying;
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    pub fn shut_down(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.engine.stop_all();
        let (journal, unsaved, chat, said) = {
            let mut state = lock(&self.state);
            let unsaved = state.activity.take_unsaved();
            let said = state.talk.take_unsaved();
            (state.activity.journal().clone(), unsaved, state.talk.journal().clone(), said)
        };
        logs::save(&journal, &unsaved);
        logs::save_talk(&chat, &said);
    }

    fn ensure_loaded(&self) {
        if lock(&self.state).loaded {
            return;
        }
        let _disk = lock(&self.disk);
        if lock(&self.state).loaded {
            return;
        }
        let now = now_ms();
        let mut activity = Activity::new(Journal::activity(&self.home));
        activity.load(now, local_offset_ms(now));
        let mut talk = Talk::new(Journal::chat(&self.home));
        talk.load();
        let mut state = lock(&self.state);
        state.activity = activity;
        state.talk = talk;
        state.loaded = true;
    }

    fn save_settings(&self, reply: &Sender<Event>) {
        let _disk = lock(&self.disk);
        let settings = lock(&self.state).settings.clone();
        if let Err(err) = settings.save(&self.home) {
            notice(reply, Tone::Error, format!("could not save Octo's settings: {err}"));
        }
    }

    pub fn relay(self: &Arc<Self>, hub: &Arc<Hub>, rescanned: bool) {
        if !self.is_relaying() {
            return;
        }
        if rescanned {
            let mut presence = lock(&self.presence);
            if presence.is_none() {
                *presence = relay::hold(&self.home).ok();
            }
        }
        let due = rescanned || self.is_enabled();
        if due && relay::has_commands(&self.home) && !self.sorting.swap(true, Ordering::SeqCst) {
            let (octo, hub) = (Arc::clone(self), Arc::clone(hub));
            let spawned = thread::Builder::new().name(INBOX_THREAD.into()).spawn(move || {
                let (dropped, _) = std::sync::mpsc::channel();
                for message in relay::take_commands(&octo.home, now_ms()) {
                    octo.command(&hub, message, &dropped);
                }
                octo.sorting.store(false, Ordering::SeqCst);
            });
            if spawned.is_err() {
                self.sorting.store(false, Ordering::SeqCst);
            }
        }
        let mut state = lock(&self.state);
        self.tidy(&mut state);
    }

    pub fn command(self: &Arc<Self>, hub: &Arc<Hub>, message: PhoneMsg, reply: &Sender<Event>) {
        match message {
            PhoneMsg::OrchestratorSet(change) => self.set(hub, change, reply),
            PhoneMsg::OrchestratorAnswer(answer) => self.answer_from_user(hub, answer, reply),
            PhoneMsg::OrchestratorChat { text } => self.chat(hub, &text, reply),
            PhoneMsg::OrchestratorClearChat => self.clear_chat(hub),
            _ => {}
        }
    }

    fn tidy(self: &Arc<Self>, state: &mut State) {
        let unsent = state.view_unsent && self.is_relaying();
        let unsaved = state.activity.has_unsaved() || state.talk.has_unsaved();
        if state.tidying || !(unsent || unsaved || !state.stops.is_empty()) {
            return;
        }
        let octo = Arc::clone(self);
        state.tidying = thread::Builder::new().name(CHORES_THREAD.into()).spawn(move || octo.do_chores()).is_ok();
    }

    fn do_chores(&self) {
        loop {
            let (stops, unsaved, said, view, journal, chat) = {
                let mut state = lock(&self.state);
                let stops = std::mem::take(&mut state.stops);
                let unsaved = state.activity.take_unsaved();
                let said = state.talk.take_unsaved();
                let unsent = std::mem::take(&mut state.view_unsent) && self.is_relaying();
                let view = unsent.then(|| state.view(now_ms()));
                if stops.is_empty() && unsaved.is_empty() && said.is_empty() && view.is_none() {
                    state.tidying = false;
                    return;
                }
                (stops, unsaved, said, view, state.activity.journal().clone(), state.talk.journal().clone())
            };
            for ticket in stops {
                self.engine.stop(ticket);
            }
            logs::save(&journal, &unsaved);
            logs::save_talk(&chat, &said);
            if let Some(view) = view {
                let _ = relay::write_view(&self.home, &view);
            }
        }
    }

    pub fn view(&self) -> OrchestratorView {
        lock(&self.state).view(now_ms())
    }

    fn publish(self: &Arc<Self>, hub: &Hub, state: &mut State) {
        if std::mem::take(&mut state.dirty) {
            hub.broadcast(&BridgeMsg::Orchestrator(state.view(now_ms())));
            state.view_unsent = self.is_relaying();
        }
        self.tidy(state);
    }

    fn start_reading(self: &Arc<Self>, hub: &Arc<Hub>, state: &mut State) {
        let unread = state.decisions.values().any(|open| open.stage == Stage::Reading);
        if unread && !state.reading {
            let (octo, reader_hub) = (Arc::clone(self), Arc::clone(hub));
            let spawned =
                thread::Builder::new().name(READER_THREAD.into()).spawn(move || octo.read_pending(&reader_hub));
            state.reading = spawned.is_ok();
        }
    }

    pub fn observe(self: &Arc<Self>, hub: &Arc<Hub>, seen: &[Sighting]) {
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        if !state.settings.enabled {
            return;
        }
        let engines = engine_views(&hub.kinds());
        if engines != state.engines {
            state.engines = engines;
            state.dirty = true;
        }
        let closed: Vec<AgentId> = state
            .decisions
            .values()
            .filter(|open| match sight_of(seen, open.decision.agent) {
                Some(Sight::Unknown) => false,
                Some(sight) => sight != open.sight,
                None => true,
            })
            .map(|open| open.decision.agent)
            .collect();
        for agent in closed {
            let Some(open) = state.decisions.remove(&agent) else { continue };
            state.stop_judging(&open);
            if open.stage == Stage::NeedsYou && sight_of(seen, agent).is_some() {
                let mode = state.settings.mode_for(&open.decision.project);
                let credit = Credit { by: By::You, outcome: Outcome::Answered, summary: ANSWERED_IN_AGENT.to_string() };
                state.record(open.handled(credit, mode));
            }
            state.dirty = true;
        }
        state.reviewed.retain(|agent, _| sight_of(seen, *agent).is_some());
        for (agent, sight) in seen {
            let (since, ended) = match *sight {
                Sight::Paused(since) => (since, false),
                Sight::Ended(since) if state.is_unreviewed(*agent, since) => (since, true),
                Sight::Ended(_) | Sight::Unknown | Sight::Other => continue,
            };
            if state.decisions.len() >= MAX_DECISIONS || state.decisions.contains_key(agent) {
                continue;
            }
            let Some(view) = hub.agent_view(*agent) else { continue };
            let replies = state.replies_in_a_row(*agent);
            if ended {
                state.reviewed.insert(*agent, Reviewed { since, replies });
            }
            let id = state.next_decision;
            state.next_decision += 1;
            let decision = Decision {
                id,
                agent: *agent,
                name: view.name,
                kind: view.kind,
                session: view.session,
                project: view.project,
                since,
                status: DecisionStatus::Deciding,
                question: String::new(),
                context: Vec::new(),
                tool: None,
                suggestion: None,
                tag: None,
                problem: None,
                ended,
            };
            let follow_ups = if ended { replies } else { 0 };
            state.open(decision, cut(view.task.trim(), MAX_TASK_CHARS), follow_ups, since, *sight);
        }
        self.start_reading(hub, state);
        self.publish(hub, state);
    }

    fn read_pending(self: &Arc<Self>, hub: &Arc<Hub>) {
        loop {
            let next = {
                let mut state = lock(&self.state);
                let next = state.decisions.values().find(|open| open.stage == Stage::Reading).map(|open| {
                    (open.decision.id, open.decision.agent, open.decision.kind, open.asked_from, open.decision.ended)
                });
                if next.is_none() {
                    state.reading = false;
                }
                next
            };
            let Some((id, agent, kind, asked_from, ended)) = next else { return };
            let screen = read_screen(hub.store(), agent);
            if ended {
                let turn = last_turn_of(hub, agent);
                self.read_turn_end(hub, id, agent, screen, turn);
                self.pump(hub);
                continue;
            }
            let ask = fresh_ask(hooks::read_ask(hub.store().home(), agent), kind, asked_from);
            let by_rule = {
                let mut guard = lock(&self.state);
                let state = &mut *guard;
                let Some(open) = state.decisions.get_mut(&agent).filter(|open| open.decision.id == id) else {
                    continue;
                };
                if let Some(screen) = screen {
                    open.decision.context = screen.rows;
                    open.app_cursor = screen.app_cursor;
                    open.bracketed = screen.bracketed;
                }
                open.decision.question = question(ask.as_ref(), &open.decision.context);
                open.decision.tool = ask.as_ref().map(tool_of);
                let mode = state.settings.mode_for(&open.decision.project);
                let exhausted = open.follow_ups >= MAX_ANSWERS_IN_A_ROW;
                let by_rule = !exhausted && allowed_by_rule(mode, kind, ask.as_ref());
                open.stage = if by_rule { Stage::Answering } else { Stage::Judging };
                if exhausted {
                    open.wait_for_user(Some(too_many_questions()));
                } else if !by_rule {
                    state.queue.push_back(Job::Judge(id));
                }
                state.dirty = true;
                self.publish(hub, state);
                by_rule
            };
            match ask.filter(|_| by_rule) {
                Some(ask) => {
                    let allowed = format!("Allowed {}: {}", ask.tool, ask.detail.lines().next().unwrap_or_default());
                    let credit = Credit {
                        by: By::Rule,
                        outcome: Outcome::Answered,
                        summary: one_line(&allowed, MAX_LINE_CHARS),
                    };
                    self.answer(hub, id, &Input::Keys(vec![ENTER.to_string()]), credit, true);
                }
                None => self.pump(hub),
            }
        }
    }

    fn read_turn_end(self: &Arc<Self>, hub: &Hub, id: u64, agent: AgentId, screen: Option<Screen>, turn: LastTurn) {
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        let Some(open) = state.decisions.get_mut(&agent).filter(|open| open.decision.id == id) else { return };
        if let Some(screen) = screen {
            open.decision.context = screen.rows;
            open.app_cursor = screen.app_cursor;
            open.bracketed = screen.bracketed;
        }
        open.decision.question = waiting_for(&turn.said, &open.decision.context);
        open.last_turn = turn;
        if open.follow_ups >= MAX_REPLIES_IN_A_ROW {
            open.wait_for_user(Some(too_many_replies()));
        } else {
            open.stage = Stage::Judging;
            state.queue.push_back(Job::Judge(id));
        }
        state.dirty = true;
        self.publish(hub, state);
    }

    fn pump(self: &Arc<Self>, hub: &Arc<Hub>) {
        loop {
            let job = {
                let mut state = lock(&self.state);
                if self.is_stopping() || state.running >= MAX_ENGINE_PROCESSES {
                    return;
                }
                let Some(job) = state.queue.pop_front() else { return };
                state.running += 1;
                job
            };
            let judged = match &job {
                Job::Judge(id) => Some(*id),
                Job::Chat { .. } => None,
            };
            let slot = Slot { octo: Arc::clone(self), hub: Arc::clone(hub) };
            let spawned = thread::Builder::new().name(ENGINE_THREAD.into()).spawn(move || match job {
                Job::Judge(id) => slot.octo.judge(&slot.hub, id),
                Job::Chat { text, reply } => slot.octo.converse(&slot.hub, &text, &reply),
            });
            if spawned.is_err() {
                self.give_up(hub, judged);
            }
        }
    }

    pub(crate) fn give_up(self: &Arc<Self>, hub: &Hub, judged: Option<u64>) {
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        match judged {
            None => state.thinking = false,
            Some(id) => {
                if let Some(open) = find(&mut state.decisions, id).filter(|open| open.stage == Stage::Judging) {
                    open.wait_for_user(Some(NO_THREAD.to_string()));
                }
            }
        }
        state.dirty = true;
        self.publish(hub, state);
    }

    fn wanted(&self, ticket: u64) -> bool {
        ticket == CHAT_TICKET || {
            let mut state = lock(&self.state);
            find(&mut state.decisions, ticket).is_some_and(|open| open.stage == Stage::Judging)
        }
    }

    fn consult<T>(
        &self,
        config: &Config,
        settings: &Settings,
        ticket: u64,
        prompt: &str,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Result<T, String> {
        let Some(primary) = settings.primary else { return Err(NO_ENGINE.to_string()) };
        if self.is_stopping() {
            return Err(STOPPED.to_string());
        }
        let ask = |kind: AgentKind, model: &Option<String>| {
            let said = self.engine.ask(config, &Call { ticket, kind, model: model.as_deref(), prompt })?;
            parse(&said).ok_or_else(|| format!("{} did not answer in the expected format", kind.label()))
        };
        match (ask(primary, &settings.primary_model), settings.secondary) {
            (Ok(answer), _) => Ok(answer),
            (Err(problem), None) => Err(problem),
            (Err(_), Some(_)) if self.is_stopping() || !self.wanted(ticket) => Err(STOPPED.to_string()),
            (Err(first), Some(secondary)) => {
                ask(secondary, &settings.secondary_model).map_err(|second| format!("{first}; {second}"))
            }
        }
    }

    fn judge(self: &Arc<Self>, hub: &Arc<Hub>, id: u64) {
        let case = {
            let mut guard = lock(&self.state);
            let state = &mut *guard;
            find(&mut state.decisions, id)
                .filter(|open| open.stage == Stage::Judging)
                .map(|open| (open.decision.clone(), open.task.clone(), open.last_turn.clone(), state.settings.clone()))
        };
        let Some((decision, task, last_turn, settings)) = case else { return };
        let (agent, name) = (decision.agent, decision.name.as_str());
        let asked_in = settings.mode_for(&decision.project);
        let global = instructions::read(&self.home, None).unwrap_or_default();
        let project = instructions::read(&self.home, Some(&decision.project)).unwrap_or_default();
        let rules = Rules { global: &global, project: &project };
        let launcher = hub.launcher();
        let config = launcher.config();
        let opinion = if decision.ended {
            let ended = TurnEnd {
                mode: asked_in,
                rules,
                decision: &decision,
                task: &task,
                asked: &last_turn.asked,
                last_message: &last_turn.said,
            };
            let read = |said: &str| verdict::first_object::<Review>(said).map(Opinion::Ended);
            self.consult(config, &settings, id, &prompt::review(&ended), read)
        } else {
            let case = Case { mode: asked_in, rules, decision: &decision, task: &task };
            let read = |said: &str| verdict::first_object::<Verdict>(said).map(Opinion::Asked);
            self.consult(config, &settings, id, &prompt::judgement(&case), read)
        };
        let mut report = String::new();
        let apply = {
            let mut guard = lock(&self.state);
            let state = &mut *guard;
            let now = state.settings.mode_for(&decision.project);
            let mode = if now == Mode::Observe { Mode::Observe } else { asked_in };
            let Some(open) = find(&mut state.decisions, id).filter(|open| open.stage == Stage::Judging) else {
                return;
            };
            let problem = opinion.as_ref().err().cloned();
            if state.problem != problem {
                state.problem = problem;
                state.dirty = true;
            }
            let ruling = opinion.map(|opinion| verdict::judge(mode, opinion)).map(
                |Judged { ruling, question, report: said }| {
                    if !question.is_empty() {
                        open.decision.question = question;
                    }
                    report = said;
                    ruling
                },
            );
            let mut finished = false;
            let (apply, logged) = match ruling {
                Ok(Ruling::Finished { summary }) => {
                    finished = true;
                    open.decision.question = FINISHED.to_string();
                    let summary = if summary.is_empty() { FINISHED.to_string() } else { summary };
                    (None, Some(Credit { by: By::Octo, outcome: Outcome::Finished, summary }))
                }
                Err(problem) => {
                    open.wait_for_user(Some(problem.clone()));
                    (None, Some(Credit { by: By::Octo, outcome: Outcome::Failed, summary: problem }))
                }
                Ok(Ruling::NeedsYou { suggestion, tag, problem }) => {
                    let summary = suggestion.as_ref().map(|suggestion| suggestion.summary.clone());
                    let summary = summary.filter(|summary| !summary.is_empty()).or_else(|| tag.clone());
                    open.decision.suggestion = suggestion;
                    open.decision.tag = tag;
                    open.wait_for_user(problem);
                    let summary = summary.unwrap_or_else(|| LEFT_FOR_YOU.to_string());
                    (None, Some(Credit { by: By::Octo, outcome: Outcome::Escalated, summary }))
                }
                Ok(Ruling::Apply { suggestion, deny }) => {
                    open.decision.suggestion = Some(suggestion.clone());
                    open.stage = Stage::Answering;
                    (Some((suggestion, deny)), None)
                }
            };
            if let Some(credit) = logged {
                let told = match credit.outcome {
                    Outcome::Finished => Some(credit.summary.clone()),
                    Outcome::Escalated => Some(format!("Left for you: {}", open.decision.question)),
                    Outcome::Answered | Outcome::Denied | Outcome::Failed => None,
                };
                let handled = open.handled(credit, mode);
                state.record(handled);
                if let Some(told) = told.filter(|_| decision.ended) {
                    state.tell(turn_note(agent, name, &report, &told), told);
                }
            }
            if finished {
                state.decisions.remove(&agent);
                state.count_reply(agent, |_| 0);
            }
            self.publish(hub, state);
            apply
        };
        let Some((suggestion, deny)) = apply else { return };
        let Some(input) = suggestion.input() else { return };
        let outcome = if deny { Outcome::Denied } else { Outcome::Answered };
        let fallback = if decision.ended { REPLIED } else { ANSWERED };
        let summary = if suggestion.summary.is_empty() { fallback.to_string() } else { suggestion.summary };
        let credit = Credit { by: By::Octo, outcome, summary: summary.clone() };
        if self.answer(hub, id, &input, credit, true) != Answered::Sent || !decision.ended {
            return;
        }
        let sent = match &input {
            Input::Text(text) => sent_line(name, text),
            Input::Keys(_) => describe(&input),
        };
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        state.count_reply(agent, |replies| replies.saturating_add(1));
        state.tell(turn_note(agent, name, &report, &summary), sent);
        self.publish(hub, state);
    }

    fn answer(self: &Arc<Self>, hub: &Arc<Hub>, id: u64, input: &Input, credit: Credit, claimed: bool) -> Answered {
        let target = {
            let mut guard = lock(&self.state);
            let state = &mut *guard;
            let (settings, stops) = (&state.settings, &mut state.stops);
            let mine = |open: &&mut Open| if claimed { open.stage == Stage::Answering } else { !open.is_taken() };
            let target = find(&mut state.decisions, id).filter(mine).map(|open| {
                if open.stage == Stage::Judging {
                    stops.push(id);
                }
                open.stage = Stage::Settled;
                let handled = open.handled(credit, settings.mode_for(&open.decision.project));
                let target = Target {
                    agent: open.decision.agent,
                    sight: open.sight,
                    app_cursor: open.app_cursor,
                    bracketed: open.bracketed,
                };
                (target, handled)
            });
            state.dirty |= target.is_some();
            self.publish(hub, state);
            target
        };
        let Some((target, handled)) = target else { return Answered::Gone };
        let (agent, sight) = (target.agent, target.sight);
        let answered = deliver(hub, &target, input);
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        let still = state.decisions.get(&agent).is_some_and(|open| open.decision.id == id);
        match &answered {
            Answered::Sent => {
                state.record(handled);
                let (octo, hub, answered_at) = (Arc::clone(self), Arc::clone(hub), now_ms());
                spawn(FOLLOW_THREAD, move || octo.follow_up(&hub, id, agent, sight, answered_at));
            }
            Answered::Failed(problem) if still => {
                if let Some(open) = state.decisions.get_mut(&agent) {
                    open.wait_for_user(Some(problem.clone()));
                    state.dirty = true;
                }
            }
            Answered::Stale | Answered::Gone if still => {
                state.decisions.remove(&agent);
            }
            Answered::Failed(_) | Answered::Stale | Answered::Gone => {}
        }
        self.publish(hub, state);
        answered
    }

    fn follow_up(self: &Arc<Self>, hub: &Arc<Hub>, id: u64, agent: AgentId, sight: Sight, answered_at: u64) {
        thread::sleep(SETTLE);
        let settled = |open: &&mut Open| open.decision.id == id && open.stage == Stage::Settled;
        let known = {
            let mut state = lock(&self.state);
            state
                .decisions
                .get_mut(&agent)
                .filter(settled)
                .map(|open| (open.decision.kind, open.decision.context.clone()))
        };
        let Some((kind, context)) = known else { return };
        if !is_still(hub, agent, sight) {
            return;
        }
        let paused = matches!(sight, Sight::Paused(_));
        let asked_again = paused && fresh_ask(hooks::read_ask(hub.store().home(), agent), kind, answered_at).is_some();
        let redrawn = || read_screen(hub.store(), agent).is_some_and(|screen| screen.rows != context);
        let moved = asked_again || (paused && redrawn());
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        let Some(answered) = state.decisions.get_mut(&agent).filter(settled) else { return };
        if !moved {
            answered.wait_for_user(Some(DID_NOT_TAKE.to_string()));
            state.dirty = true;
            return self.publish(hub, state);
        }
        let (task, follow_ups) = (answered.task.clone(), answered.follow_ups.saturating_add(1));
        let decision = Decision {
            id: state.next_decision,
            status: DecisionStatus::Deciding,
            question: String::new(),
            context: Vec::new(),
            tool: None,
            suggestion: None,
            tag: None,
            problem: None,
            ..answered.decision.clone()
        };
        state.next_decision += 1;
        state.open(decision, task, follow_ups, answered_at, sight);
        self.start_reading(hub, state);
        self.publish(hub, state);
    }

    fn converse(self: &Arc<Self>, hub: &Arc<Hub>, message: &str, reply: &Sender<Event>) {
        let (settings, decisions, mut history) = {
            let state = lock(&self.state);
            (state.settings.clone(), state.pending(), state.talk.last(MAX_CHAT_HISTORY + 1))
        };
        if history.last().is_some_and(|entry| entry.role == ChatRole::You && entry.text == message) {
            history.pop();
        }
        let global = instructions::read(&self.home, None).unwrap_or_default();
        let agents = hub.agent_views();
        let sessions = session_names(hub.store());
        let models = start_models(&hub.kinds());
        let talk = prompt::Talk {
            mode: settings.mode,
            project_modes: &settings.projects,
            watching: settings.enabled,
            global: &global,
            sessions: &sessions,
            models: &models,
            agents: &agents,
            decisions: &decisions,
            history: &history,
            message,
        };
        let first = prompt::chat(&talk);
        let launcher = hub.launcher();
        let config = launcher.config();
        let answer = self.consult(config, &settings, CHAT_TICKET, &first, chat_answer).and_then(|answer| {
            let reads = answer.reads();
            if reads.is_empty() {
                return Ok(answer);
            }
            let screens: Vec<prompt::Screen> = reads
                .into_iter()
                .map(|agent| prompt::Screen {
                    agent,
                    rows: read_screen(hub.store(), agent).map(|screen| screen.rows).unwrap_or_default(),
                    last_message: last_turn_of(hub, agent).said,
                })
                .collect();
            self.consult(config, &settings, CHAT_TICKET, &prompt::chat_with_screens(&first, &screens), chat_answer)
        });
        let entry = match &answer {
            Ok(answer) => ChatEntry {
                at: now_ms(),
                role: ChatRole::Octo,
                text: answer.said(),
                actions: answer.actions().into_iter().map(|action| self.act(hub, action, reply)).collect(),
                failed: false,
            },
            Err(problem) => ChatEntry {
                at: now_ms(),
                role: ChatRole::Octo,
                text: problem.clone(),
                actions: Vec::new(),
                failed: true,
            },
        };
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        state.talk.say(entry);
        state.thinking = false;
        state.problem = answer.err();
        state.dirty = true;
        self.publish(hub, state);
    }

    fn act(self: &Arc<Self>, hub: &Arc<Hub>, action: Option<ChatAction>, reply: &Sender<Event>) -> String {
        let name_of = |agent: AgentId| hub.record(agent).map_or_else(|| format!("#{agent}"), |record| record.name);
        match action {
            None => WRONG_ACTION.to_string(),
            Some(ChatAction::Send { agent, text }) => {
                let name = name_of(agent);
                if !hub.state(agent).is_some_and(AgentState::is_alive) {
                    return format!("Could not send to {name}: it is not running");
                }
                let text = cut(text.trim(), MAX_CHAT_CHARS);
                let (app_cursor, bracketed) = read_screen(hub.store(), agent)
                    .map_or((false, false), |screen| (screen.app_cursor, screen.bracketed));
                match type_input(hub, agent, &Input::Text(text.clone()), app_cursor, bracketed) {
                    Ok(()) => sent_line(&name, &text),
                    Err(err) => format!("Could not send to {name}: {err}"),
                }
            }
            Some(ChatAction::Answer { decision, keys, text }) => {
                let Some(input) = usable_input(keys, text) else {
                    return format!("Could not answer decision {decision}: nothing to type");
                };
                let summary = describe(&input);
                let credit = Credit { by: By::Octo, outcome: Outcome::Answered, summary: summary.clone() };
                match self.answer(hub, decision, &input, credit, false) {
                    Answered::Sent => format!("Answered decision {decision}: {summary}"),
                    Answered::Stale => format!("Decision {decision} was already answered in the agent"),
                    Answered::Gone => format!("Decision {decision} is no longer pending"),
                    Answered::Failed(problem) => format!("Could not answer decision {decision}: {problem}"),
                }
            }
            Some(ChatAction::Stop { agent }) => {
                let name = name_of(agent);
                let stopped = hub.send(agent, &ClientMsg::Kill);
                hub.nudge();
                match stopped {
                    Ok(()) => format!("Stopping {name}"),
                    Err(err) => format!("Could not stop {name}: {err}"),
                }
            }
            Some(ChatAction::Start { kind, cwd, task, session, model }) => {
                let Some(kind) = AgentKind::parse(&kind) else {
                    return format!("Could not start {kind}: it is not an agent Octo knows");
                };
                let label = kind.label();
                let folder = actions::resolve_dir(&cwd);
                let requested = session.as_deref().map(str::trim).filter(|session| !session.is_empty());
                let sessions = session_names(hub.store());
                let remembered = hub.store().load_ui().session;
                let Some(session) = session_for(requested, &folder, &sessions, &hub.agent_views(), &remembered) else {
                    return format!("Could not start {label}: there is no session to put it in");
                };
                let request = StartRequest {
                    kind,
                    session,
                    cwd: folder.to_string_lossy().into_owned(),
                    task,
                    worktree: false,
                    bypass: false,
                    remote_control: false,
                    model,
                    attachments: Vec::new(),
                };
                let launch = match actions::prepare(hub, request) {
                    Ok(launch) => launch,
                    Err(err) => return format!("Could not start {label}: {err}"),
                };
                match launch.run(hub) {
                    Ok(record) => started(&record),
                    Err(err) => format!("Could not start {label} in {}: {err}", folder.display()),
                }
            }
            Some(ChatAction::Restart { agent }) => {
                let name = name_of(agent);
                let Some(mut record) = hub.record(agent) else {
                    return format!("Could not restart {name}: there is no such agent");
                };
                if !actions::is_stopped(hub, agent) {
                    return format!("Could not restart {name}: it is still running");
                }
                let Some(_relaunch) = hub.try_relaunch(agent) else { return format!("{name} is already starting") };
                let restarted = hub.launcher().start(&mut record, LaunchMode::Resume, DEFAULT_SIZE);
                hub.nudge();
                match restarted {
                    Ok(()) => format!("Restarted {name}"),
                    Err(err) => format!("Could not restart {name}: {err}"),
                }
            }
            Some(ChatAction::Remove { agent }) => {
                let name = name_of(agent);
                let Some(record) = hub.record(agent) else {
                    return format!("Could not remove {name}: there is no such agent");
                };
                if !actions::is_stopped(hub, agent) {
                    return format!("Could not remove {name}: stop it first");
                }
                match actions::remove_stopped(hub, record, reply) {
                    Ok(()) => format!("Removed {name}"),
                    Err(err) => format!("Could not remove {name}: {err}"),
                }
            }
            Some(ChatAction::Rename { agent, name: wanted }) => {
                let name = name_of(agent);
                match actions::rename_agent(hub, agent, &wanted) {
                    Ok(Some((old, new))) => format!("Renamed {old} to {new}"),
                    Ok(None) => format!("Could not rename {name}: there is no such agent"),
                    Err(err) => format!("Could not rename {name}: {err}"),
                }
            }
        }
    }

    pub fn show(&self, hub: &Hub, reply: &Sender<Event>) {
        self.ensure_loaded();
        let mut state = lock(&self.state);
        state.engines = engine_views(&hub.kinds());
        let _ = reply.send(Event::Reply(BridgeMsg::Orchestrator(state.view(now_ms()))));
    }

    pub fn set(self: &Arc<Self>, hub: &Hub, change: SettingsChange, reply: &Sender<Event>) {
        if change.enabled == Some(true) {
            self.ensure_loaded();
        }
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        let was_enabled = state.settings.enabled;
        match state.settings.apply(change) {
            Err(problem) => return notice(reply, Tone::Error, problem),
            Ok(false) => return,
            Ok(true) => {}
        }
        let enabled = state.settings.enabled;
        self.enabled.store(enabled, Ordering::Relaxed);
        self.wake();
        if enabled && !was_enabled {
            state.watching_since = now_ms();
        }
        if !enabled {
            state.reviewed.clear();
            let judging = state.decisions.values().filter(|open| open.stage == Stage::Judging);
            state.stops.extend(judging.map(|open| open.decision.id));
            state.decisions.clear();
            state.queue.retain(|job| matches!(job, Job::Chat { .. }));
        }
        state.engines = engine_views(&hub.kinds());
        state.dirty = true;
        self.publish(hub, state);
        drop(guard);
        self.save_settings(reply);
        if enabled && !was_enabled {
            hub.nudge();
        }
    }

    pub fn answer_from_user(self: &Arc<Self>, hub: &Arc<Hub>, answer: UserAnswer, reply: &Sender<Event>) {
        let UserAnswer { decision: id, how, text, keys, remember } = answer;
        let (project, typed) = {
            let mut guard = lock(&self.state);
            let state = &mut *guard;
            let Some(open) = find(&mut state.decisions, id).filter(|open| !open.is_taken()) else {
                return notice(reply, Tone::Warn, NOT_PENDING);
            };
            let typed = match typed_by(how, text, keys, open.decision.suggestion.as_ref()) {
                Ok(typed) => typed,
                Err(warning) => return notice(reply, Tone::Warn, warning),
            };
            if open.stage == Stage::Judging {
                state.stops.push(id);
            }
            open.stage = if typed.is_some() { Stage::Answering } else { Stage::Settled };
            let project = open.decision.project.clone();
            state.dirty |= typed.is_none();
            self.publish(hub, state);
            (project, typed)
        };
        let rule = remember.map(|rule| rule.trim().to_string()).filter(|rule| !rule.is_empty());
        if typed.is_none() && rule.is_none() {
            return;
        }
        let claimed = typed.is_some();
        let (octo, job_hub, job_reply) = (Arc::clone(self), Arc::clone(hub), reply.clone());
        let started = hub.jobs().run(ANSWER_THREAD, reply, move || {
            if let Some(rule) = rule
                && let Err(err) = instructions::remember(&octo.home, &project, &rule)
            {
                notice(&job_reply, Tone::Error, format!("could not remember that rule: {err}"));
            }
            let Some((input, summary)) = typed else { return };
            let credit = Credit { by: By::You, outcome: Outcome::Answered, summary };
            match octo.answer(&job_hub, id, &input, credit, true) {
                Answered::Sent => {}
                Answered::Stale => notice(&job_reply, Tone::Info, "the agent had already moved on"),
                Answered::Gone => notice(&job_reply, Tone::Warn, NOT_PENDING),
                Answered::Failed(problem) => notice(&job_reply, Tone::Error, problem),
            }
        });
        if claimed && !started {
            let mut state = lock(&self.state);
            if let Some(open) = find(&mut state.decisions, id).filter(|open| open.stage == Stage::Answering) {
                let problem = open.decision.problem.take();
                open.wait_for_user(problem);
            }
        }
    }

    pub fn chat(self: &Arc<Self>, hub: &Arc<Hub>, text: &str, reply: &Sender<Event>) {
        let text = cut(text.trim(), MAX_CHAT_CHARS);
        if text.is_empty() {
            return;
        }
        self.ensure_loaded();
        {
            let mut guard = lock(&self.state);
            let state = &mut *guard;
            if state.thinking {
                return notice(reply, Tone::Warn, "Octo is still answering your last message");
            }
            let said =
                ChatEntry { at: now_ms(), role: ChatRole::You, text: text.clone(), actions: Vec::new(), failed: false };
            state.talk.say(said);
            state.reviewed.values_mut().for_each(|reviewed| reviewed.replies = 0);
            state.thinking = true;
            state.queue.push_back(Job::Chat { text, reply: reply.clone() });
            state.dirty = true;
            self.publish(hub, state);
        }
        self.pump(hub);
    }

    pub fn clear_chat(self: &Arc<Self>, hub: &Hub) {
        let mut state = lock(&self.state);
        state.dirty |= !state.talk.is_empty();
        state.talk.clear();
        self.publish(hub, &mut state);
    }
}

pub fn instructions(hub: &Hub, project: Option<String>, reply: &Sender<Event>) {
    let (home, job_reply) = (hub.store().home().clone(), reply.clone());
    hub.jobs().run(RULES_THREAD, reply, move || match instructions::read(&home, project.as_deref()) {
        Ok(text) => {
            let path = instructions::file(&home, project.as_deref()).to_string_lossy().into_owned();
            let _ = job_reply.send(Event::Reply(BridgeMsg::OrchestratorInstructions { project, text, path }));
        }
        Err(err) => notice(&job_reply, Tone::Error, format!("could not read Octo's instructions: {err}")),
    });
}

pub fn save_instructions(hub: &Hub, project: Option<String>, text: String, reply: &Sender<Event>) {
    let (home, job_reply) = (hub.store().home().clone(), reply.clone());
    hub.jobs().run(RULES_THREAD, reply, move || match instructions::save(&home, project.as_deref(), &text) {
        Ok(path) => {
            let path = path.to_string_lossy().into_owned();
            let _ = job_reply.send(Event::Reply(BridgeMsg::OrchestratorInstructions { project, text, path }));
        }
        Err(err) => notice(&job_reply, Tone::Error, format!("could not save Octo's instructions: {err}")),
    });
}

pub fn activity(hub: &Hub, before: Option<u64>, reply: &Sender<Event>) {
    let (journal, job_reply) = (Journal::activity(hub.store().home()), reply.clone());
    hub.jobs().run(ACTIVITY_THREAD, reply, move || {
        let (items, next) = logs::page(&journal, before);
        let _ = job_reply.send(Event::Reply(BridgeMsg::OrchestratorActivity { items, next }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(id: AgentId, session: &str, cwd: &Path) -> AgentView {
        AgentView {
            id,
            session: session.into(),
            name: format!("agent-{id}"),
            kind: AgentKind::Claude,
            state: AgentState::Idle,
            starting: false,
            activity: String::new(),
            task: String::new(),
            cwd: cwd.to_string_lossy().into_owned(),
            project: String::new(),
            color: String::new(),
            started_at: None,
            state_since: None,
            turn_started: None,
            last_turn: None,
            branch: None,
            exit: None,
            changes: None,
            test: None,
            subagents: Vec::new(),
            model: None,
        }
    }

    #[test]
    fn a_new_agent_goes_to_a_session_that_exists_even_when_the_remembered_one_is_gone() {
        let home = std::env::temp_dir();
        let (limes, other) = (home.join("limes"), home.join("other"));
        let sessions = ["teste".to_string(), "work".to_string()];
        let agents = [placed(3, "teste", &other), placed(5, "work", &limes), placed(4, "teste", &limes)];
        let pick = |requested: Option<&str>, cwd: &Path, remembered: &str| {
            session_for(requested, cwd, &sessions, &agents, remembered)
        };
        assert_eq!(
            pick(Some("teste"), &limes, "default").as_deref(),
            Some("teste"),
            "the one asked for, when it exists"
        );
        assert_eq!(
            pick(Some("ghost"), &limes, "default").as_deref(),
            Some("work"),
            "else the newest agent's beside it"
        );
        assert_eq!(pick(None, &limes, "default").as_deref(), Some("work"));
        assert_eq!(pick(None, &home.join("new"), "work").as_deref(), Some("work"), "else the remembered one");
        assert_eq!(
            pick(None, &home.join("new"), "default").as_deref(),
            Some("teste"),
            "a remembered session that is gone falls back to the first one"
        );
        assert_eq!(session_for(None, &limes, &[], &agents, "default"), None, "no session, nowhere to start");
    }

    #[test]
    fn the_engines_are_claude_and_codex_whether_installed_or_not() {
        assert_eq!(
            engine_views(&[AgentKind::Codex, AgentKind::Kimi, AgentKind::Terminal]),
            [
                EngineView { kind: AgentKind::Claude, installed: false },
                EngineView { kind: AgentKind::Codex, installed: true }
            ]
        );
        assert!(engine_views(&[]).iter().all(|engine| !engine.installed));
    }

    #[test]
    fn an_agent_is_looked_up_in_the_sorted_list_the_hub_hands_over() {
        let seen = [(2, Sight::Other), (5, Sight::Paused(40)), (9, Sight::Unknown)];
        assert_eq!(sight_of(&seen, 5), Some(Sight::Paused(40)));
        assert_eq!(sight_of(&seen, 9), Some(Sight::Unknown));
        assert_eq!(sight_of(&seen, 2), Some(Sight::Other));
        assert_eq!(sight_of(&seen, 3), None, "an agent that was removed is not in the list");
        assert_eq!(sight_of(&[], 1), None);
    }

    #[test]
    fn a_chat_answer_written_as_prose_is_still_a_reply() {
        let read = chat_answer(r#"{"reply":"All quiet.","read":[3]}"#).unwrap();
        assert_eq!((read.said().as_str(), read.reads()), ("All quiet.", vec![3]));
        let prose = chat_answer("  Nothing is running right now.  ").unwrap();
        assert_eq!(prose.said(), "Nothing is running right now.");
        assert!(prose.reads().is_empty() && prose.actions().is_empty());
        let braces = chat_answer("Use {} for an empty object.").unwrap();
        assert_eq!(braces.said(), "Use {} for an empty object.", "an object that says nothing is not the answer");
        assert_eq!(chat_answer("   "), None);
        assert_eq!(chat_answer(""), None);
    }

    #[test]
    fn the_answer_of_the_user_becomes_what_is_typed_or_a_reason_it_cannot_be() {
        let keys = |names: &[&str]| Some(names.iter().map(|name| (*name).to_string()).collect::<Vec<_>>());
        let suggested = Suggestion::of(Input::Keys(vec!["2".into()]), "Refuse it".into(), String::new());
        assert_eq!(typed_by(How::Dismiss, Some("x".into()), None, Some(&suggested)), Ok(None));
        assert_eq!(
            typed_by(How::Allow, None, None, None),
            Ok(Some((Input::Keys(vec!["Enter".into()]), "Allowed it".to_string())))
        );
        assert_eq!(
            typed_by(How::Text, Some(" keep it ".into()), None, None),
            Ok(Some((Input::Text("keep it".into()), "Typed: keep it".to_string())))
        );
        assert_eq!(typed_by(How::Text, Some("  ".into()), None, None), Err("write what to answer"));
        assert_eq!(
            typed_by(How::Keys, None, keys(&["1"]), None),
            Ok(Some((Input::Keys(vec!["1".into()]), "Pressed 1".to_string())))
        );
        assert_eq!(typed_by(How::Keys, None, keys(&["Bogus"]), None), Err("those keys cannot be pressed"));
        assert_eq!(
            typed_by(How::Suggestion, None, None, Some(&suggested)),
            Ok(Some((Input::Keys(vec!["2".into()]), "Refuse it".to_string())))
        );
        let unnamed = Suggestion { summary: String::new(), ..suggested };
        assert_eq!(
            typed_by(How::Suggestion, None, None, Some(&unnamed)),
            Ok(Some((Input::Keys(vec!["2".into()]), "Pressed 2".to_string())))
        );
        assert_eq!(typed_by(How::Suggestion, None, None, None), Err("Octo has no suggestion for this one"));
    }

    #[test]
    fn the_limit_of_answers_in_a_row_is_the_one_its_message_names() {
        assert!(too_many_questions().contains(&format!("answered {MAX_ANSWERS_IN_A_ROW} questions")));
        assert!(too_many_replies().contains(&format!("replied {MAX_REPLIES_IN_A_ROW} times in a row")));
    }

    #[test]
    fn a_turn_end_is_told_in_the_chat_with_the_name_of_the_agent_and_what_octo_reported() {
        assert_eq!(
            turn_note(41, "api", "  Ele terminou o deploy.  ", "Deployed it"),
            "api (#41): Ele terminou o deploy."
        );
        assert_eq!(turn_note(41, "api", "  ", "Fixed the login"), "api (#41): Fixed the login");
    }

    #[test]
    fn what_an_agent_waits_for_is_the_last_line_it_said_or_what_its_screen_asks() {
        let rows = vec!["Do you want to proceed?".to_string(), "\u{276f} 1. Yes".to_string()];
        assert_eq!(waiting_for("Done with the API.\n\nShould I deploy it?\n  \n", &rows), "Should I deploy it?");
        assert_eq!(waiting_for("  \n", &rows), "Do you want to proceed?", "nothing said: the screen is all there is");
        assert_eq!(waiting_for("", &[]), "");
        assert_eq!(waiting_for(&"x".repeat(MAX_QUESTION_CHARS * 2), &[]).chars().count(), MAX_QUESTION_CHARS);
    }

    #[test]
    fn only_four_jobs_asked_by_a_window_run_at_once_and_the_next_is_told_octo_is_busy() {
        let jobs = Jobs::default();
        let (reply, replies) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let held = Arc::new(Mutex::new(held));
        let (done, finished) = std::sync::mpsc::channel();
        let job = || {
            let (held, done) = (Arc::clone(&held), done.clone());
            move || {
                let _ = lock(&held).recv();
                let _ = done.send(());
            }
        };
        for _ in 0..MAX_CLIENT_JOBS {
            assert!(jobs.run("orkai-test-job", &reply, job()));
        }
        assert!(!jobs.run("orkai-test-job", &reply, job()), "the fifth does not start");
        let Ok(Event::Reply(BridgeMsg::Notice { text, tone })) = replies.try_recv() else { panic!("no notice") };
        assert_eq!((tone, text.as_str()), (Tone::Warn, "Octo is busy; try again"));
        assert_eq!(jobs.running.load(Ordering::SeqCst), MAX_CLIENT_JOBS, "a refused job does not count");
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(20)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while jobs.running.load(Ordering::SeqCst) == MAX_CLIENT_JOBS && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(jobs.run("orkai-test-job", &reply, job()), "room was made by the one that ended");
        assert!(replies.try_recv().is_err());
        drop(release);
    }

    #[test]
    fn a_message_sent_to_an_agent_is_shown_whole_or_visibly_shortened() {
        assert_eq!(sent_line("api", "run the\ntests"), "Sent to api: run the tests");
        let exact = "x".repeat(MAX_SENT_CHARS);
        assert_eq!(sent_line("api", &exact), format!("Sent to api: {exact}"), "nothing was left out");
        let long = sent_line("api", &"y".repeat(MAX_SENT_CHARS + 1));
        assert!(long.ends_with("y…"), "what is shown shortened says so, so it is not taken for what was sent");
        assert_eq!(long.chars().count(), "Sent to api: ".len() + MAX_SENT_CHARS + 1);
    }

    #[test]
    fn what_was_sent_is_described_in_one_short_line() {
        assert_eq!(describe(&Input::Keys(vec!["ArrowDown".into(), "Enter".into()])), "Pressed ArrowDown Enter");
        assert_eq!(describe(&Input::Text("keep the\nold endpoint".into())), "Typed: keep the old endpoint");
        let long = describe(&Input::Text("x".repeat(MAX_LINE_CHARS * 2)));
        assert_eq!(long.chars().count(), "Typed: ".len() + MAX_LINE_CHARS);
    }
}
