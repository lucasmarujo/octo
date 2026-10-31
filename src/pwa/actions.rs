use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;

use crate::cli::DEFAULT_SIZE;
use crate::launch::LaunchMode;
use crate::model::{AgentId, AgentState, NewAgent, validate_agent_name};
use crate::paths::{expand_tilde, matching_directories, user_home};
use crate::protocol::ClientMsg;
use crate::pwa::hub::{Event, Hub};
use crate::pwa::protocol::{BridgeMsg, Dir, PhoneMsg, StartRequest, Tone};
use crate::tui::app::merge_message;
use crate::tui::format::default_agent_name;
use crate::worktree;

pub const MAX_DIFF_BYTES: usize = 512 * 1024;

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
        PhoneMsg::Sync
        | PhoneMsg::Open { .. }
        | PhoneMsg::Close
        | PhoneMsg::Size { .. }
        | PhoneMsg::Keys { .. }
        | PhoneMsg::Send { .. }
        | PhoneMsg::Paste { .. }
        | PhoneMsg::Key { .. }
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
    let StartRequest { kind, session, cwd, task, worktree, bypass } = request;
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
    let place = if isolate { " in a new worktree" } else { "" };
    let mode = if bypass { " with permissions bypassed" } else { "" };
    notice(reply, Tone::Info, format!("starting {} {name}{place}{mode}…", kind.label()));
    let draft = NewAgent { session, name, kind, cwd, task };
    let (hub, reply) = (Arc::clone(hub), reply.clone());
    spawn("orkai-pwa-launch", move || {
        match hub.launcher().create(draft, DEFAULT_SIZE, isolate, bypass) {
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
    let (hub, reply) = (Arc::clone(hub), reply.clone());
    spawn("orkai-pwa-recover", move || {
        for mut record in records {
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
    let Some(worktree) = hub.record(agent).and_then(|record| record.worktree) else {
        return notice(reply, Tone::Warn, "this agent does not run in its own worktree");
    };
    let reply = reply.clone();
    spawn("orkai-pwa-diff", move || match worktree::diff(&worktree) {
        Ok(bytes) => {
            let (text, truncated) = clip(&bytes, MAX_DIFF_BYTES);
            let _ = reply.send(Event::Reply(BridgeMsg::Diff { agent, branch: worktree.branch, text, truncated }));
        }
        Err(err) => notice(&reply, Tone::Error, format!("could not read the changes: {err}")),
    });
}

fn merge(hub: &Arc<Hub>, agent: AgentId, reply: &Sender<Event>) {
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
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
