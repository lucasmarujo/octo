use std::fs;
use std::io;

use crate::fsutil;
use crate::model::{
    AgentId, AgentRecord, DEFAULT_SESSION, ExitInfo, LaunchSpec, NewAgent, RuntimeInfo, Session, SessionList, UiState,
    validate_agent_name, validate_session_name,
};
use crate::paths::{Home, create_private_dir};
use crate::util::now_ms;

#[derive(Clone, Debug)]
pub struct Store {
    home: Home,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

impl Store {
    pub fn open(home: Home) -> io::Result<Self> {
        home.ensure()?;
        Ok(Self { home })
    }

    pub fn home(&self) -> &Home {
        &self.home
    }

    pub fn load_sessions(&self) -> io::Result<Vec<Session>> {
        let records = self.list_agents()?;
        self.load_sessions_with(&records)
    }

    pub fn load_sessions_with(&self, records: &[AgentRecord]) -> io::Result<Vec<Session>> {
        let (sessions, changed) = self.peek_sessions_with(records)?;
        if changed {
            self.save_sessions(&sessions)?;
        }
        Ok(sessions)
    }

    pub fn peek_sessions_with(&self, records: &[AgentRecord]) -> io::Result<(Vec<Session>, bool)> {
        let mut list = fsutil::read_json_opt::<SessionList>(&self.home.sessions_file())?.unwrap_or_default();
        let mut changed = false;
        if !list.sessions.iter().any(|s| s.name == DEFAULT_SESSION) {
            list.sessions.insert(0, Session { name: DEFAULT_SESSION.to_string(), created_at_ms: now_ms() });
            changed = true;
        }
        for record in records {
            if !list.sessions.iter().any(|s| s.name == record.session) {
                list.sessions.push(Session { name: record.session.clone(), created_at_ms: record.created_at_ms });
                changed = true;
            }
        }
        Ok((list.sessions, changed))
    }

    pub fn save_sessions(&self, sessions: &[Session]) -> io::Result<()> {
        fsutil::write_json_atomic(&self.home.sessions_file(), &SessionList { sessions: sessions.to_vec() })
    }

    pub fn create_session(&self, name: &str) -> io::Result<Session> {
        let name = validate_session_name(name).map_err(invalid)?;
        let mut sessions = self.load_sessions()?;
        if sessions.iter().any(|s| s.name == name) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("session '{name}' already exists")));
        }
        let session = Session { name, created_at_ms: now_ms() };
        sessions.push(session.clone());
        self.save_sessions(&sessions)?;
        Ok(session)
    }

    pub fn next_session_name(&self) -> io::Result<String> {
        let sessions = self.load_sessions()?;
        let name = (2..)
            .map(|n| format!("session-{n}"))
            .find(|candidate| !sessions.iter().any(|s| &s.name == candidate))
            .unwrap_or_else(|| "session".to_string());
        Ok(name)
    }

    pub fn rename_session(&self, old: &str, new: &str) -> io::Result<()> {
        let new = validate_session_name(new).map_err(invalid)?;
        let mut sessions = self.load_sessions()?;
        if sessions.iter().any(|s| s.name == new) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("session '{new}' already exists")));
        }
        let Some(session) = sessions.iter_mut().find(|s| s.name == old) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("session '{old}' not found")));
        };
        session.name = new.clone();
        for mut record in self.list_agents()?.into_iter().filter(|r| r.session == old) {
            record.session = new.clone();
            self.save_agent(&record)?;
        }
        self.save_sessions(&sessions)
    }

    pub fn delete_session(&self, name: &str) -> io::Result<()> {
        if name == DEFAULT_SESSION {
            return Err(invalid("the default session cannot be deleted"));
        }
        if self.list_agents()?.iter().any(|r| r.session == name) {
            return Err(invalid(format!("session '{name}' still has agents")));
        }
        let mut sessions = self.load_sessions()?;
        sessions.retain(|s| s.name != name);
        self.save_sessions(&sessions)
    }

    pub fn list_agents(&self) -> io::Result<Vec<AgentRecord>> {
        let mut records = Vec::new();
        let entries = match fs::read_dir(self.home.agents_dir()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(records),
            Err(err) => return Err(err),
        };
        for entry in entries.flatten() {
            let Some(id) = entry.file_name().to_str().and_then(|n| n.parse::<AgentId>().ok()) else {
                continue;
            };
            if let Ok(Some(record)) = fsutil::read_json_opt::<AgentRecord>(&self.home.agent_file(id))
                && record.id == id
            {
                records.push(record);
            }
        }
        records.sort_by_key(|r| r.id);
        Ok(records)
    }

    pub fn load_agent(&self, id: AgentId) -> io::Result<AgentRecord> {
        fsutil::read_json(&self.home.agent_file(id))
    }

    fn max_agent_id(&self) -> io::Result<AgentId> {
        let entries = match fs::read_dir(self.home.agents_dir()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(err),
        };
        Ok(entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<AgentId>().ok()))
            .max()
            .unwrap_or(0))
    }

    pub fn create_agent(&self, draft: NewAgent) -> io::Result<AgentRecord> {
        let name = validate_agent_name(&draft.name).map_err(invalid)?;
        let session = validate_session_name(&draft.session).map_err(invalid)?;
        let mut id = self.max_agent_id()?.saturating_add(1);
        loop {
            match fs::create_dir(self.home.agent_dir(id)) {
                Ok(()) => break,
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => id = id.saturating_add(1),
                Err(err) => return Err(err),
            }
        }
        create_private_dir(&self.home.agent_dir(id))?;
        let record = AgentRecord {
            id,
            session,
            name,
            kind: draft.kind,
            cwd: draft.cwd,
            task: draft.task,
            created_at_ms: now_ms(),
            conversation_id: None,
            launches: 0,
            worktree: None,
            bypass: false,
        };
        self.save_agent(&record)?;
        Ok(record)
    }

    pub fn save_agent(&self, record: &AgentRecord) -> io::Result<()> {
        fsutil::write_json_atomic(&self.home.agent_file(record.id), record)
    }

    pub fn remove_agent(&self, id: AgentId) -> io::Result<()> {
        fsutil::remove_dir_all_retry(&self.home.agent_dir(id))
    }

    pub fn write_launch(&self, id: AgentId, spec: &LaunchSpec) -> io::Result<()> {
        fsutil::write_json_atomic(&self.home.launch_file(id), spec)
    }

    pub fn read_runtime(&self, id: AgentId) -> io::Result<Option<RuntimeInfo>> {
        fsutil::read_json_opt(&self.home.runtime_file(id))
    }

    pub fn read_exit(&self, id: AgentId) -> io::Result<Option<ExitInfo>> {
        fsutil::read_json_opt(&self.home.exit_file(id))
    }

    pub fn clear_runtime_markers(&self, id: AgentId) -> io::Result<()> {
        fsutil::remove_file_if_exists(&self.home.runtime_file(id))?;
        fsutil::remove_file_if_exists(&self.home.exit_file(id))
    }

    pub fn load_ui(&self) -> UiState {
        fsutil::read_json_opt::<UiState>(&self.home.ui_file()).ok().flatten().unwrap_or_default()
    }

    pub fn save_ui(&self, state: &UiState) -> io::Result<()> {
        fsutil::write_json_atomic(&self.home.ui_file(), state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentKind;
    use std::path::PathBuf;

    fn temp_store(tag: &str) -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("orkai-store-{tag}-{}-{}", std::process::id(), now_ms()));
        (Store::open(Home::at(&dir)).unwrap(), dir)
    }

    fn draft(session: &str, name: &str) -> NewAgent {
        NewAgent {
            session: session.into(),
            name: name.into(),
            kind: AgentKind::Claude,
            cwd: PathBuf::from("/work/app"),
            task: "do it".into(),
        }
    }

    #[test]
    fn default_session_always_exists() {
        let (store, dir) = temp_store("default");
        let sessions = store.load_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, DEFAULT_SESSION);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sessions_can_be_created_renamed_and_deleted() {
        let (store, dir) = temp_store("sessions");
        assert_eq!(store.next_session_name().unwrap(), "session-2");
        store.create_session("session-2").unwrap();
        assert_eq!(store.next_session_name().unwrap(), "session-3");
        assert!(store.create_session("session-2").is_err());
        let agent = store.create_agent(draft("session-2", "a")).unwrap();
        store.rename_session("session-2", "work").unwrap();
        assert_eq!(store.load_agent(agent.id).unwrap().session, "work");
        assert!(store.delete_session("work").is_err());
        store.remove_agent(agent.id).unwrap();
        store.delete_session("work").unwrap();
        assert!(store.delete_session(DEFAULT_SESSION).is_err());
        let names: Vec<_> = store.load_sessions().unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec![DEFAULT_SESSION]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn agent_ids_are_sequential_and_unique() {
        let (store, dir) = temp_store("ids");
        let a = store.create_agent(draft("default", "a")).unwrap();
        let b = store.create_agent(draft("default", "b")).unwrap();
        assert_eq!((a.id, b.id), (1, 2));
        fs::create_dir(store.home().agent_dir(3)).unwrap();
        let c = store.create_agent(draft("default", "c")).unwrap();
        assert_eq!(c.id, 4);
        let ids: Vec<_> = store.list_agents().unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![1, 2, 4]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn agents_in_unknown_sessions_register_the_session() {
        let (store, dir) = temp_store("orphan");
        store.create_agent(draft("recovered", "a")).unwrap();
        let names: Vec<_> = store.load_sessions().unwrap().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"recovered".to_string()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_names_are_rejected() {
        let (store, dir) = temp_store("invalid");
        assert!(store.create_agent(draft("bad session", "a")).is_err());
        assert!(store.create_agent(draft("default", "")).is_err());
        assert!(store.create_session("../x").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remove_agent_deletes_its_directory() {
        let (store, dir) = temp_store("remove");
        let a = store.create_agent(draft("default", "a")).unwrap();
        store.remove_agent(a.id).unwrap();
        assert!(!store.home().agent_dir(a.id).exists());
        assert!(store.list_agents().unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn peeking_at_the_sessions_never_writes_them() {
        let (store, dir) = temp_store("peek");
        let record = store.create_agent(draft("work", "a")).unwrap();
        assert!(!store.home().sessions_file().exists());
        let (sessions, changed) = store.peek_sessions_with(std::slice::from_ref(&record)).unwrap();
        assert!(changed);
        assert_eq!(sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), [DEFAULT_SESSION, "work"]);
        assert!(!store.home().sessions_file().exists(), "a peek leaves the file to its writers");
        store.load_sessions_with(&[record]).unwrap();
        assert!(store.home().sessions_file().exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ui_state_round_trips_and_defaults() {
        let (store, dir) = temp_store("ui");
        assert_eq!(store.load_ui(), UiState::default());
        let state = UiState {
            session: "work".into(),
            who: AgentKind::Codex,
            where_dir: Some(PathBuf::from("/x")),
            tabs: vec![3, 1],
            session_dirs: [("work".to_string(), PathBuf::from("/x/y"))].into(),
            worktree: true,
            bypass: true,
            pwa: true,
        };
        store.save_ui(&state).unwrap();
        assert_eq!(store.load_ui(), state);
        fs::remove_dir_all(dir).unwrap();
    }
}
