use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;

use crate::cli::DEFAULT_SIZE;
use crate::client;
use crate::launch;
use crate::launch::LaunchMode;
use crate::model::{AgentId, AgentKind, AgentRecord, AgentState, NewAgent, validate_agent_name};
use crate::paths::{expand_tilde, matching_directories, user_home};
use crate::project;
use crate::protocol::ClientMsg;
use crate::pwa::hub::{Event, Hub};
use crate::pwa::protocol::{
    BridgeMsg, CommitView, Details, Dir, FileView, OverlapView, PhoneMsg, Preview, Run, SkillView, StartRequest,
    SubagentView, TestView, Tone,
};
use crate::pwa::screen;
use crate::skills;
use crate::store::Store;
use crate::subagents::{self, SubagentWatcher};
use crate::tui::app::{
    MAX_RACERS, first_line, merge_message, parse_race, race_header, read_tail, review_task, reviewer_for,
};
use crate::tui::format::default_agent_name;
use crate::util::now_ms;
use crate::worktree;

pub const MAX_DIFF_BYTES: usize = 512 * 1024;
pub const MAX_PEEK_AGENTS: usize = 24;
pub const MAX_PEEK_ROWS: u16 = 40;
const DISCARD_WAIT: std::time::Duration = std::time::Duration::from_secs(20);
const DISCARD_POLL: std::time::Duration = std::time::Duration::from_millis(100);

fn notice(reply: &Sender<Event>, tone: Tone, text: impl Into<String>) {
    let _ = reply.send(Event::Reply(BridgeMsg::Notice { text: text.into(), tone }));
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    let _ = thread::Builder::new().name(name.to_string()).spawn(work);
}

pub fn handle(hub: &Arc<Hub>, message: PhoneMsg, reply: &Sender<Event>) {
    match message {
        PhoneMsg::Stop { agent } => stop(hub, agent, reply),
        PhoneMsg::Remove { agent } => remove(hub, agent, reply),
        PhoneMsg::Seen { agent } => {
            let _ = hub.send(agent, &ClientMsg::Seen);
        }
        PhoneMsg::Rename { agent, name } => rename(hub, agent, &name, reply),
        PhoneMsg::Start(request) => start(hub, request, reply),
        PhoneMsg::Session { name } => match hub.store().create_session(&name) {
            Ok(session) => {
                hub.nudge();
                notice(reply, Tone::Success, format!("session {} created", session.name));
            }
            Err(err) => notice(reply, Tone::Error, err.to_string()),
        },
        PhoneMsg::Recover { session } => recover(hub, &session, reply),
        PhoneMsg::Folders { path } => folders(&path, reply),
        PhoneMsg::Diff { agent } => diff(hub, agent, reply),
        PhoneMsg::Merge { agent } => merge(hub, agent, reply),
        PhoneMsg::Peek { agents, rows } => peek(hub, agents, rows, reply),
        PhoneMsg::Details { agent } => details(hub, agent, reply),
        PhoneMsg::Restart { agent } => restart(hub, agent, reply),
        PhoneMsg::CrossReview { agent, reviewer } => cross_review(hub, agent, &reviewer, reply),
        PhoneMsg::Tests { agent } => tests(hub, agent, reply),
        PhoneMsg::Skills { kind, cwd } => list_skills(kind, &cwd, reply),
        PhoneMsg::Skill { kind, session, cwd, name, args, worktree, bypass, remote_control } => {
            run_skill(hub, SkillRun { kind, session, cwd, name, args, worktree, bypass, remote_control }, reply);
        }
        PhoneMsg::Race { session, cwd, race: line, bypass, remote_control } => {
            race(hub, session, &cwd, &line, bypass, remote_control, reply);
        }
        PhoneMsg::Compare { agent } => compare(hub, agent, reply),
        PhoneMsg::Pick { agent } => pick(hub, agent, reply),
        PhoneMsg::RenameSession { name, to } => match hub.store().rename_session(&name, &to) {
            Ok(()) => {
                hub.nudge();
                notice(reply, Tone::Success, format!("session {name} is now {}", to.trim()));
            }
            Err(err) => notice(reply, Tone::Error, err.to_string()),
        },
        PhoneMsg::DeleteSession { name } => match hub.store().delete_session(&name) {
            Ok(()) => {
                hub.nudge();
                notice(reply, Tone::Info, format!("session {name} deleted"));
            }
            Err(err) => notice(reply, Tone::Error, err.to_string()),
        },
        PhoneMsg::Trust { cwd, command } => trust(hub, &cwd, command, reply),
        PhoneMsg::Sync
        | PhoneMsg::Open { .. }
        | PhoneMsg::Close
        | PhoneMsg::Size { .. }
        | PhoneMsg::Keys { .. }
        | PhoneMsg::Send { .. }
        | PhoneMsg::Paste { .. }
        | PhoneMsg::Key { .. }
        | PhoneMsg::LinkAt { .. }
        | PhoneMsg::Scroll { .. } => {}
    }
}

fn name_of(hub: &Hub, agent: AgentId) -> Option<String> {
    hub.record(agent).map(|record| record.name)
}

fn stop(hub: &Hub, agent: AgentId, reply: &Sender<Event>) {
    let name = name_of(hub, agent).unwrap_or_else(|| format!("#{agent}"));
    match hub.send(agent, &ClientMsg::Kill) {
        Ok(()) => notice(reply, Tone::Info, format!("stopping {name}…")),
        Err(err) => notice(reply, Tone::Error, format!("could not stop {name}: {err}")),
    }
    hub.nudge();
}

fn remove(hub: &Arc<Hub>, agent: AgentId, reply: &Sender<Event>) {
    let Some(record) = hub.record(agent) else { return };
    if !matches!(hub.state(agent), Some(AgentState::Exited | AgentState::Lost)) {
        return notice(reply, Tone::Warn, format!("stop {} before removing it", record.name));
    }
    if let Err(err) = hub.store().remove_agent(agent) {
        return notice(reply, Tone::Error, format!("could not remove {}: {err}", record.name));
    }
    hub.nudge();
    notice(reply, Tone::Info, format!("{} removed", record.name));
    if let Some(worktree) = record.worktree {
        let (home, reply, name) = (hub.store().home().clone(), reply.clone(), record.name);
        spawn("orkai-pwa-discard", move || {
            if let Err(err) = worktree::discard(&home, &worktree) {
                notice(&reply, Tone::Error, format!("could not delete the worktree of {name}: {err}"));
            }
        });
    }
}

fn rename(hub: &Hub, agent: AgentId, name: &str, reply: &Sender<Event>) {
    let name = match validate_agent_name(name) {
        Ok(name) => name,
        Err(err) => return notice(reply, Tone::Error, err),
    };
    let Ok(mut record) = hub.store().load_agent(agent) else { return };
    let old = std::mem::replace(&mut record.name, name.clone());
    match hub.store().save_agent(&record) {
        Ok(()) => {
            hub.nudge();
            notice(reply, Tone::Success, format!("{old} is now {name}"));
        }
        Err(err) => notice(reply, Tone::Error, format!("rename failed: {err}")),
    }
}

pub fn resolve_dir(path: &str) -> PathBuf {
    let path = path.trim();
    if path.is_empty() {
        return user_home().unwrap_or_else(|| PathBuf::from("."));
    }
    let expanded = expand_tilde(path);
    let absolute = if expanded.is_relative() { user_home().unwrap_or_default().join(expanded) } else { expanded };
    absolute.components().collect()
}

fn start(hub: &Arc<Hub>, request: StartRequest, reply: &Sender<Event>) {
    let StartRequest { kind, session, cwd, task, worktree, bypass, remote_control } = request;
    if !hub.kinds().contains(&kind) {
        return notice(reply, Tone::Error, format!("{} is not installed on {}", kind.label(), hub.host()));
    }
    let cwd = resolve_dir(&cwd);
    if !cwd.is_dir() {
        return notice(reply, Tone::Error, format!("{} is not a folder", cwd.display()));
    }
    if !hub.store().load_sessions().is_ok_and(|sessions| sessions.iter().any(|s| s.name == session)) {
        return notice(reply, Tone::Error, format!("session {session} does not exist"));
    }
    let name = default_agent_name(kind, &task, |name| hub.name_taken(&session, name));
    let isolate = worktree && kind.is_ai();
    let bypass = bypass && kind.is_ai();
    let remote_control = remote_control && kind == AgentKind::Claude;
    let place = if isolate { " in a new worktree" } else { "" };
    let mode = if bypass { " with permissions bypassed" } else { "" };
    notice(reply, Tone::Info, format!("starting {} {name}{place}{mode}…", kind.label()));
    let draft = NewAgent { session, name, kind, cwd, task };
    let (hub, reply) = (Arc::clone(hub), reply.clone());
    spawn("orkai-pwa-launch", move || {
        match hub.launcher().create(draft, DEFAULT_SIZE, isolate, bypass, remote_control) {
            Ok(record) => notice(&reply, Tone::Success, format!("{} {} started", record.kind.label(), record.name)),
            Err(err) => notice(&reply, Tone::Error, format!("could not start the agent: {err}")),
        }
        hub.nudge();
    });
}

fn recover(hub: &Arc<Hub>, session: &str, reply: &Sender<Event>) {
    let records = hub.lost_in(session);
    if records.is_empty() {
        return notice(reply, Tone::Info, "nothing to recover in this session");
    }
    notice(reply, Tone::Info, format!("recovering {} agent(s)…", records.len()));
    relaunch(hub, records, reply);
}

pub fn recover_all(hub: &Arc<Hub>, reply: &Sender<Event>) -> usize {
    let records = hub.lost();
    let count = records.len();
    if count > 0 {
        notice(reply, Tone::Info, format!("recovering {count} interrupted agent(s)…"));
        relaunch(hub, records, reply);
    }
    count
}

fn restart(hub: &Arc<Hub>, agent: AgentId, reply: &Sender<Event>) {
    let Some(record) = hub.record(agent) else { return };
    if !matches!(hub.state(agent), Some(AgentState::Exited | AgentState::Lost)) {
        return notice(reply, Tone::Error, format!("{} is still running", record.name));
    }
    notice(reply, Tone::Info, format!("restarting {}…", record.name));
    relaunch(hub, vec![record], reply);
}

fn relaunch(hub: &Arc<Hub>, records: Vec<AgentRecord>, reply: &Sender<Event>) {
    let mut claimed = Vec::new();
    for record in records {
        match hub.try_relaunch(record.id) {
            Some(guard) => claimed.push((record, guard)),
            None => notice(reply, Tone::Info, format!("{} is already starting", record.name)),
        }
    }
    let (hub, reply) = (Arc::clone(hub), reply.clone());
    spawn("orkai-pwa-recover", move || {
        for (mut record, _guard) in claimed {
            if let Err(err) = hub.launcher().start(&mut record, LaunchMode::Resume, DEFAULT_SIZE) {
                notice(&reply, Tone::Error, format!("could not recover {}: {err}", record.name));
            }
        }
        hub.nudge();
    });
}

pub fn list_folders(path: &str) -> BridgeMsg {
    let dir = resolve_dir(path);
    let dirs = matching_directories(&dir, "")
        .into_iter()
        .map(|name| Dir { path: dir.join(&name).to_string_lossy().into_owned(), name })
        .collect();
    BridgeMsg::Folders {
        path: dir.to_string_lossy().into_owned(),
        parent: dir.parent().map(|parent| parent.to_string_lossy().into_owned()),
        dirs,
    }
}

fn folders(path: &str, reply: &Sender<Event>) {
    let (path, reply) = (path.to_string(), reply.clone());
    spawn("orkai-pwa-folders", move || {
        let _ = reply.send(Event::Reply(list_folders(&path)));
    });
}

fn cross_review(hub: &Arc<Hub>, agent: AgentId, reviewer: &str, reply: &Sender<Event>) {
    let Some(record) = hub.record(agent) else { return };
    let Some(worktree) = record.worktree.as_ref() else {
        return notice(reply, Tone::Error, format!("{} has no worktree to review", record.name));
    };
    let reviewed = hub.live_status(agent).map_or(record.kind, |status| status.running_kind(record.kind));
    let kinds = hub.kinds();
    let kind = if reviewer.trim().is_empty() {
        match reviewer_for(reviewed, &kinds) {
            Some(kind) => kind,
            None => return notice(reply, Tone::Error, "install an AI agent to review the work"),
        }
    } else {
        match AgentKind::parse(reviewer).filter(|kind| kind.is_ai()) {
            Some(kind) => kind,
            None => return notice(reply, Tone::Error, format!("{reviewer} cannot review: name an AI agent")),
        }
    };
    start(
        hub,
        StartRequest {
            kind,
            session: record.session.clone(),
            cwd: record.cwd.to_string_lossy().into_owned(),
            task: review_task(worktree),
            worktree: false,
            bypass: false,
            remote_control: false,
        },
        reply,
    );
}

struct SkillRun {
    kind: AgentKind,
    session: String,
    cwd: String,
    name: String,
    args: String,
    worktree: bool,
    bypass: bool,
    remote_control: bool,
}

fn list_skills(kind: AgentKind, cwd: &str, reply: &Sender<Event>) {
    let (dir, cwd, reply) = (resolve_dir(cwd), cwd.to_string(), reply.clone());
    spawn("orkai-pwa-skills", move || {
        let skills = skills::discover(kind, &dir)
            .into_iter()
            .map(|skill| SkillView { name: skill.name, owner: skill.owner })
            .collect();
        let _ = reply.send(Event::Reply(BridgeMsg::Skills { kind, cwd, skills }));
    });
}

fn run_skill(hub: &Arc<Hub>, run: SkillRun, reply: &Sender<Event>) {
    let found = skills::discover(run.kind, &resolve_dir(&run.cwd))
        .into_iter()
        .find(|skill| skill.name.eq_ignore_ascii_case(&run.name));
    let Some(skill) = found else {
        return notice(reply, Tone::Error, format!("unknown command /{} — try /help", run.name));
    };
    let task = skills::invocation(run.kind, &skill, &run.args);
    let request = StartRequest {
        kind: run.kind,
        session: run.session,
        cwd: run.cwd,
        task,
        worktree: run.worktree,
        bypass: run.bypass,
        remote_control: run.remote_control,
    };
    start(hub, request, reply);
}

pub fn race_kinds(kinds: &[AgentKind], installed: &[AgentKind]) -> Result<(), String> {
    if kinds.len() < 2 || kinds.len() > MAX_RACERS {
        return Err(format!("a race takes 2 to {MAX_RACERS} agents"));
    }
    match kinds.iter().find(|kind| !kind.is_ai() || !installed.contains(kind)) {
        Some(kind) => Err(format!("{} cannot race here", kind.label())),
        None => Ok(()),
    }
}

fn race(
    hub: &Arc<Hub>,
    session: String,
    cwd: &str,
    line: &str,
    bypass: bool,
    remote_control: bool,
    reply: &Sender<Event>,
) {
    let Some((kinds, task)) = parse_race(line) else {
        return notice(reply, Tone::Error, format!("usage: /race <agent,agent…> <task>, 2 to {MAX_RACERS} AI agents"));
    };
    if let Err(err) = race_kinds(&kinds, &hub.kinds()) {
        return notice(reply, Tone::Error, err);
    }
    let cwd = resolve_dir(cwd);
    if let Err(err) = worktree::toplevel(&cwd) {
        return notice(reply, Tone::Error, format!("{err}; each agent of a race works in its own worktree"));
    }
    let race = now_ms();
    let mut chosen: Vec<String> = Vec::new();
    let mut drafts = Vec::new();
    for kind in kinds {
        let text = format!("{} {task}", kind.label());
        let name =
            default_agent_name(kind, &text, |name| hub.name_taken(&session, name) || chosen.iter().any(|c| c == name));
        chosen.push(name.clone());
        drafts.push(NewAgent { session: session.clone(), name, kind, cwd: cwd.clone(), task: task.clone() });
    }
    notice(reply, Tone::Info, format!("racing {} in their own worktrees…", chosen.join(", ")));
    let (hub, reply) = (Arc::clone(hub), reply.clone());
    spawn("orkai-pwa-race", move || {
        for draft in drafts {
            let bypass = bypass && draft.kind.is_ai();
            let remote_control = remote_control && draft.kind == AgentKind::Claude;
            let name = draft.name.clone();
            if let Err(err) = hub.launcher().create_in_race(draft, DEFAULT_SIZE, bypass, remote_control, race) {
                notice(&reply, Tone::Error, format!("could not start {name}: {err}"));
            }
            hub.nudge();
        }
    });
}

fn compare(hub: &Arc<Hub>, agent: AgentId, reply: &Sender<Event>) {
    let members = hub.race_members(agent);
    if members.len() < 2 {
        return notice(reply, Tone::Error, "this agent is not racing others (start a race with /race)");
    }
    let title =
        members.iter().find(|record| record.id == agent).map(|record| first_line(&record.task)).unwrap_or_default();
    let entries: Vec<(String, crate::model::Worktree)> = members
        .into_iter()
        .filter_map(|record| {
            let status = hub.live_status(record.id);
            let kind = status.as_ref().map_or(record.kind, |status| status.running_kind(record.kind));
            let state = hub.state(record.id).unwrap_or(AgentState::Idle);
            let header =
                race_header(record.id, &record.name, kind, state, status.as_ref().and_then(|s| s.test.as_deref()));
            Some((header, record.worktree?))
        })
        .collect();
    let reply = reply.clone();
    spawn("orkai-pwa-compare", move || {
        let mut out = Vec::new();
        for (header, worktree) in entries {
            out.extend_from_slice(header.as_bytes());
            match worktree::summary(&worktree) {
                Ok(summary) if summary.is_empty() => out.extend_from_slice(b" no changes yet\n"),
                Ok(summary) => out.extend_from_slice(&summary),
                Err(err) => out.extend_from_slice(format!(" could not read the changes: {err}\n").as_bytes()),
            }
            out.extend_from_slice(b"\n");
        }
        let (text, _) = clip(&out, MAX_DIFF_BYTES);
        let _ = reply.send(Event::Reply(BridgeMsg::Compare { agent, title, text }));
    });
}

fn pick(hub: &Arc<Hub>, agent: AgentId, reply: &Sender<Event>) {
    let members = hub.race_members(agent);
    if members.len() < 2 {
        return notice(reply, Tone::Error, "this agent is not racing others (start a race with /race)");
    }
    let losers: Vec<AgentId> = members.iter().map(|record| record.id).filter(|id| *id != agent).collect();
    merge_then(hub, agent, reply, move |hub, reply| {
        for loser in losers {
            discard(hub, loser, reply);
        }
    });
}

fn discard(hub: &Arc<Hub>, agent: AgentId, reply: &Sender<Event>) {
    if matches!(hub.state(agent), Some(AgentState::Exited | AgentState::Lost)) {
        return remove(hub, agent, reply);
    }
    let _ = hub.send(agent, &ClientMsg::Kill);
    let (hub, reply) = (Arc::clone(hub), reply.clone());
    spawn("orkai-pwa-discard-loser", move || {
        let deadline = std::time::Instant::now() + DISCARD_WAIT;
        while std::time::Instant::now() < deadline {
            if matches!(hub.state(agent), Some(AgentState::Exited | AgentState::Lost)) {
                return remove(&hub, agent, &reply);
            }
            thread::sleep(DISCARD_POLL);
        }
        notice(&reply, Tone::Warn, format!("#{agent} did not stop; remove it by hand"));
    });
}

fn trust(hub: &Hub, cwd: &str, confirmed: Option<String>, reply: &Sender<Event>) {
    let repo = match worktree::toplevel(&resolve_dir(cwd)) {
        Ok(repo) => repo,
        Err(err) => return notice(reply, Tone::Error, err.to_string()),
    };
    let shown = repo.display().to_string();
    let loaded = match project::load(&repo) {
        Ok(Some(loaded)) => loaded,
        Ok(None) => return notice(reply, Tone::Info, format!("{shown} has no {}", project::FILE)),
        Err(err) => return notice(reply, Tone::Error, err.to_string()),
    };
    let Some(command) = loaded.config.test_command().map(str::to_string) else {
        return notice(reply, Tone::Info, format!("the {} of {shown} has no test command", project::FILE));
    };
    if project::is_trusted(hub.store().home(), &loaded) {
        return notice(reply, Tone::Info, format!("orkai already runs {command} for {shown}"));
    }
    match confirmed {
        None => {
            let _ = reply.send(Event::Reply(BridgeMsg::TrustAsk { cwd: cwd.to_string(), repo: shown, command }));
        }
        Some(asked) if asked != command => {
            notice(reply, Tone::Error, format!("the test command of {shown} changed; check it again"));
        }
        Some(_) => match project::trust(hub.store().home(), &loaded) {
            Ok(()) => notice(reply, Tone::Success, format!("agents in worktrees now run {command} after each turn")),
            Err(err) => notice(reply, Tone::Error, format!("could not trust {}: {err}", loaded.path.display())),
        },
    }
}

fn tests(hub: &Hub, agent: AgentId, reply: &Sender<Event>) {
    let Some(record) = hub.record(agent) else { return };
    let Some(test) = hub.live_status(agent).and_then(|status| status.test) else {
        return notice(reply, Tone::Info, format!("{} has not run tests yet", record.name));
    };
    let path = hub.store().home().test_log(agent);
    let (store_path, reply) = (path, reply.clone());
    spawn("orkai-pwa-tests", move || {
        let truncated = std::fs::metadata(&store_path).is_ok_and(|meta| meta.len() > MAX_DIFF_BYTES as u64);
        let text = String::from_utf8_lossy(&read_tail(&store_path, MAX_DIFF_BYTES)).into_owned();
        let _ = reply.send(Event::Reply(BridgeMsg::Tests {
            agent,
            command: test.command,
            state: test.state,
            text,
            truncated,
        }));
    });
}

pub fn overlaps_with(
    agent: AgentId,
    mine: &worktree::Changes,
    others: &[(AgentId, String, worktree::Changes)],
) -> Vec<OverlapView> {
    others
        .iter()
        .filter(|(id, _, _)| *id != agent)
        .filter_map(|(id, name, changes)| {
            let paths: Vec<String> = mine.paths.intersection(&changes.paths).cloned().collect();
            (!paths.is_empty()).then(|| OverlapView { agent: *id, name: name.clone(), paths })
        })
        .collect()
}

fn describe(hub: &Hub, record: &AgentRecord) -> Details {
    let agent = record.id;
    let status = hub.live_status(agent);
    let state = hub.state(agent).unwrap_or(AgentState::Idle);
    let kind = status.as_ref().map_or(record.kind, |status| status.running_kind(record.kind));
    let parent = status.as_ref().and_then(|status| subagents::parent_of(record, kind, status.nested.as_ref(), state));
    let subagents = parent
        .map(|parent| SubagentWatcher::new(launch::claude_home(), launch::codex_home()).scan(&[parent], now_ms()))
        .and_then(|mut found| found.remove(&agent))
        .unwrap_or_default()
        .into_iter()
        .map(|sub| SubagentView {
            role: sub.role,
            description: sub.description,
            activity: sub.activity,
            started_at: sub.started_at_ms,
        })
        .collect();
    let mut details = Details {
        agent,
        subagents,
        test: status.and_then(|status| status.test).map(|test| TestView {
            state: test.state,
            command: test.command,
            started_at: test.started_at_ms,
            finished_at: test.finished_at_ms,
            code: test.code,
        }),
        ..Details::default()
    };
    let Some(mine) = record.worktree.as_ref() else {
        let folder = worktree::folder(&record.cwd).ok();
        details.changes = folder.as_ref().and_then(|folder| worktree::pending(folder).ok()).map(file_views);
        details.commits = folder.and_then(|folder| worktree::log(&folder.repo, None).ok()).map(commit_views);
        return details;
    };
    details.commits = worktree::log(&mine.path, Some(&mine.base)).ok().map(commit_views);
    let Ok(files) = worktree::files(mine) else { return details };
    let changes = worktree::Changes {
        files: files.len(),
        insertions: files.iter().map(|file| file.insertions).sum(),
        deletions: files.iter().map(|file| file.deletions).sum(),
        paths: files.iter().flat_map(|file| file.renamed_from.iter().chain([&file.path])).cloned().collect(),
    };
    let repo = crate::paths::normalize_for_compare(&mine.repo);
    let others: Vec<(AgentId, String, worktree::Changes)> = hub
        .worktrees()
        .into_iter()
        .filter(|(id, _, worktree)| *id != agent && crate::paths::normalize_for_compare(&worktree.repo) == repo)
        .filter_map(|(id, name, worktree)| worktree::changes(&worktree).ok().map(|changes| (id, name, changes)))
        .collect();
    details.overlaps = overlaps_with(agent, &changes, &others);
    details.changes = Some(file_views(files));
    details
}

fn file_views(files: Vec<worktree::FileChange>) -> Vec<FileView> {
    files
        .into_iter()
        .map(|file| FileView { path: file.path, insertions: file.insertions, deletions: file.deletions })
        .collect()
}

fn commit_views(commits: Vec<worktree::Commit>) -> Vec<CommitView> {
    commits
        .into_iter()
        .map(|commit| CommitView {
            hash: commit.hash,
            subject: commit.subject,
            author: commit.author,
            at: commit.at_ms,
            on_branch: commit.on_branch,
        })
        .collect()
}

fn details(hub: &Arc<Hub>, agent: AgentId, reply: &Sender<Event>) {
    let Some(record) = hub.record(agent) else { return };
    let (hub, reply) = (Arc::clone(hub), reply.clone());
    spawn("orkai-pwa-details", move || {
        let _ = reply.send(Event::Reply(BridgeMsg::Details(describe(&hub, &record))));
    });
}

pub fn peek_bounds(agents: Vec<AgentId>, rows: u16) -> (Vec<AgentId>, u16) {
    (agents.into_iter().take(MAX_PEEK_AGENTS).collect(), rows.clamp(1, MAX_PEEK_ROWS))
}

pub fn tail(screen: &vt100::Screen, rows: u16) -> Vec<Vec<Run>> {
    let (height, _) = screen.size();
    let lines: Vec<Vec<Run>> = (0..height).map(|row| screen::row(screen, row)).collect();
    let end = lines.iter().rposition(|runs| !runs.is_empty()).map_or(0, |last| last + 1);
    lines[end.saturating_sub(usize::from(rows))..end].to_vec()
}

fn preview(store: &Store, agent: AgentId, rows: u16) -> Option<Preview> {
    let runtime = store.read_runtime(agent).ok().flatten()?;
    let snapshot = client::peek(&runtime).ok()?;
    let mut parser = vt100::Parser::new(snapshot.size.rows, snapshot.size.cols, 0);
    parser.process(&snapshot.data);
    Some(Preview { agent, lines: tail(parser.screen(), rows) })
}

fn peek(hub: &Hub, agents: Vec<AgentId>, rows: u16, reply: &Sender<Event>) {
    let (agents, rows) = peek_bounds(agents, rows);
    let (store, reply) = (hub.store().clone(), reply.clone());
    spawn("orkai-pwa-peek", move || {
        let previews = thread::scope(|scope| {
            let jobs: Vec<_> = agents.iter().map(|agent| scope.spawn(|| preview(&store, *agent, rows))).collect();
            jobs.into_iter().filter_map(|job| job.join().ok().flatten()).collect()
        });
        let _ = reply.send(Event::Reply(BridgeMsg::Peek { previews }));
    });
}

pub fn clip(text: &[u8], limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (String::from_utf8_lossy(text).into_owned(), false);
    }
    let mut end = limit;
    while end > 0 && (text[end] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    (String::from_utf8_lossy(&text[..end]).into_owned(), true)
}

fn diff(hub: &Hub, agent: AgentId, reply: &Sender<Event>) {
    let Some(record) = hub.record(agent) else { return };
    let reply = reply.clone();
    spawn("orkai-pwa-diff", move || {
        let read = match record.worktree {
            Some(worktree) => worktree::diff(&worktree).map(|bytes| (worktree.branch, bytes)),
            None => worktree::folder(&record.cwd)
                .and_then(|folder| worktree::pending_diff(&folder).map(|bytes| (folder.branch, bytes))),
        };
        match read {
            Ok((branch, bytes)) => {
                let (text, truncated) = clip(&bytes, MAX_DIFF_BYTES);
                let _ = reply.send(Event::Reply(BridgeMsg::Diff { agent, branch, text, truncated }));
            }
            Err(err) => notice(&reply, Tone::Error, format!("could not read the changes: {err}")),
        }
    });
}

fn merge(hub: &Arc<Hub>, agent: AgentId, reply: &Sender<Event>) {
    match hub.record(agent) {
        Some(record) if record.worktree.is_none() => commit(hub, record, reply),
        _ => merge_then(hub, agent, reply, |_, _| {}),
    }
}

fn commit(hub: &Hub, record: AgentRecord, reply: &Sender<Event>) {
    if matches!(hub.state(record.id), Some(AgentState::Working | AgentState::Paused)) {
        return notice(reply, Tone::Warn, format!("wait for {} to finish (or stop it) before committing", record.name));
    }
    notice(reply, Tone::Info, format!("committing what is pending in the folder of {}…", record.name));
    let (reply, message) = (reply.clone(), merge_message(&record));
    spawn("orkai-pwa-commit", move || {
        match worktree::folder(&record.cwd).and_then(|folder| worktree::commit(&folder, &message)) {
            Ok(Some(done)) => notice(&reply, Tone::Success, format!("committed to {} as {}", done.branch, done.tip)),
            Ok(None) => notice(&reply, Tone::Info, "nothing is pending to commit"),
            Err(err) => notice(&reply, Tone::Error, format!("commit failed: {err}")),
        }
    });
}

fn merge_then(
    hub: &Arc<Hub>,
    agent: AgentId,
    reply: &Sender<Event>,
    after: impl FnOnce(&Arc<Hub>, &Sender<Event>) + Send + 'static,
) {
    let Some(record) = hub.record(agent) else { return };
    let Some(worktree) = record.worktree.clone() else {
        return notice(reply, Tone::Warn, "this agent does not run in its own worktree");
    };
    if matches!(hub.state(agent), Some(AgentState::Working | AgentState::Paused)) {
        return notice(reply, Tone::Warn, format!("wait for {} to finish (or stop it) before merging", record.name));
    }
    notice(reply, Tone::Info, format!("merging {}…", worktree.branch));
    let (hub, reply, message) = (Arc::clone(hub), reply.clone(), merge_message(&record));
    spawn("orkai-pwa-merge", move || {
        let merged = match worktree::merge(&worktree, &message) {
            Ok(merged) => merged,
            Err(err) => return notice(&reply, Tone::Error, format!("merge failed: {err}")),
        };
        let store = hub.store();
        let saved = store.load_agent(agent).and_then(|mut record| {
            if let Some(worktree) = record.worktree.as_mut() {
                worktree.base = merged.tip.clone();
            }
            store.save_agent(&record)
        });
        let text = match saved {
            Err(err) => {
                return notice(&reply, Tone::Error, format!("{} was merged, but not recorded: {err}", worktree.branch));
            }
            Ok(()) if merged.up_to_date => format!("{} had nothing new for {}", worktree.branch, merged.target),
            Ok(()) => format!("{} merged into {}", worktree.branch, merged.target),
        };
        notice(&reply, Tone::Success, text);
        hub.nudge();
        after(&hub, &reply);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[Vec<Run>]) -> Vec<String> {
        lines.iter().map(|runs| runs.iter().map(|run| run.0.as_str()).collect()).collect()
    }

    #[test]
    fn a_preview_keeps_the_last_lines_and_drops_the_blank_rows_below_them() {
        let mut parser = vt100::Parser::new(10, 40, 0);
        parser.process(b"one\r\ntwo\r\n\r\nfour\r\nfive");
        assert_eq!(texts(&tail(parser.screen(), 3)), ["", "four", "five"]);
        assert_eq!(texts(&tail(parser.screen(), 9)), ["one", "two", "", "four", "five"]);
        assert!(tail(vt100::Parser::new(10, 40, 0).screen(), 4).is_empty());
    }

    #[test]
    fn a_peek_asks_for_a_bounded_number_of_agents_and_lines() {
        let (agents, rows) = peek_bounds((1..=100).collect(), 500);
        assert_eq!(agents.len(), MAX_PEEK_AGENTS);
        assert_eq!(rows, MAX_PEEK_ROWS);
        assert_eq!(peek_bounds(vec![7], 0), (vec![7], 1));
    }

    #[test]
    fn a_race_takes_two_to_four_installed_ai_agents() {
        let installed = [AgentKind::Claude, AgentKind::Codex, AgentKind::Terminal];
        assert!(race_kinds(&[AgentKind::Claude, AgentKind::Codex], &installed).is_ok());
        assert!(
            race_kinds(&[AgentKind::Claude, AgentKind::Claude], &installed).is_ok(),
            "the same agent can race itself"
        );
        assert!(race_kinds(&[AgentKind::Claude], &installed).unwrap_err().contains("2 to 4"));
        assert!(race_kinds(&[AgentKind::Claude; 5], &installed).is_err());
        assert!(race_kinds(&[AgentKind::Claude, AgentKind::Terminal], &installed).unwrap_err().contains("terminal"));
        assert!(race_kinds(&[AgentKind::Claude, AgentKind::Kimi], &installed).unwrap_err().contains("kimi"));
    }

    #[test]
    fn the_radar_names_the_agents_that_touch_the_same_files() {
        let changes = |paths: &[&str]| worktree::Changes {
            files: paths.len(),
            insertions: 0,
            deletions: 0,
            paths: paths.iter().map(|path| (*path).to_string()).collect(),
        };
        let mine = changes(&["src/engine.rs", "src/mod.rs"]);
        let others = [
            (1, "me".to_string(), changes(&["src/engine.rs"])),
            (2, "physics".to_string(), changes(&["src/engine.rs", "README.md"])),
            (3, "docs".to_string(), changes(&["README.md"])),
        ];
        assert_eq!(
            overlaps_with(1, &mine, &others),
            [OverlapView { agent: 2, name: "physics".into(), paths: vec!["src/engine.rs".into()] }],
            "the agent itself and agents on other files stay out"
        );
    }

    #[test]
    fn a_long_diff_is_cut_on_a_character_boundary() {
        let text = "aé".repeat(10);
        let (cut, truncated) = clip(text.as_bytes(), 4);
        assert!(truncated);
        assert_eq!(cut, "aéa");
        assert_eq!(clip(b"short", 10), ("short".to_string(), false));
    }

    #[test]
    fn an_empty_or_relative_folder_starts_at_home() {
        let home = user_home().unwrap();
        assert_eq!(resolve_dir(""), home);
        assert_eq!(resolve_dir("dev"), home.join("dev"));
        let absolute = std::env::temp_dir();
        assert_eq!(resolve_dir(&absolute.to_string_lossy()), absolute.components().collect::<PathBuf>());
        assert_eq!(resolve_dir("~/dev/app"), home.join("dev").join("app"));
    }

    #[test]
    fn the_folder_listing_names_the_subfolders_and_the_parent() {
        let dir =
            std::env::temp_dir().join(format!("orkai-pwa-folders-{}-{}", std::process::id(), crate::util::now_ms()));
        std::fs::create_dir_all(dir.join("beta")).unwrap();
        std::fs::create_dir_all(dir.join("Alpha")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("file.txt"), "x").unwrap();
        let BridgeMsg::Folders { path, parent, dirs } = list_folders(&dir.to_string_lossy()) else { panic!() };
        assert_eq!(path, dir.to_string_lossy());
        assert_eq!(parent.as_deref(), dir.parent().map(|p| p.to_string_lossy()).as_deref());
        let names: Vec<&str> = dirs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta"]);
        assert_eq!(dirs[1].path, dir.join("beta").to_string_lossy());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
