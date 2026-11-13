use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::model::{AgentId, AgentKind};
use crate::orchestrator::{
    ACTIVITY_PAGE, ActivityEntry, By, ChatEntry, MAX_ACTIVITY, MAX_CHAT, Mode, Outcome, ProjectCount,
};
use crate::paths::{Home, create_private_dir};
use crate::util::day_of;

pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
pub const MAX_PROJECT_COUNTS: usize = 64;
const ACTIVITY_FILE: &str = "activity.jsonl";
const CHAT_FILE: &str = "chat.jsonl";
pub const MAX_UNSAVED: usize = 256;
const ROTATED_EXTENSION: &str = "old.jsonl";

pub fn push_bounded<T>(items: &mut VecDeque<T>, item: T, limit: usize) {
    while items.len() >= limit.max(1) {
        items.pop_front();
    }
    items.push_back(item);
}

fn open_for_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                create_private_dir(parent)?;
            }
            options.open(path)
        }
        opened => opened,
    }
}

fn entries_of<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    let mut data = Vec::new();
    if file.take(MAX_LOG_BYTES * 2).read_to_end(&mut data).is_err() {
        return Vec::new();
    }
    data.split(|byte| *byte == b'\n').filter_map(|line| serde_json::from_slice(line).ok()).collect()
}

#[derive(Clone, Debug)]
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn activity(home: &Home) -> Self {
        Self::new(home.orchestrator_dir().join(ACTIVITY_FILE))
    }

    pub fn chat(home: &Home) -> Self {
        Self::new(home.orchestrator_dir().join(CHAT_FILE))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rotated(&self) -> PathBuf {
        self.path.with_extension(ROTATED_EXTENSION)
    }

    pub fn append<T: Serialize>(&self, entry: &T) -> io::Result<()> {
        if fs::metadata(&self.path).is_ok_and(|meta| meta.len() >= MAX_LOG_BYTES) {
            fs::rename(&self.path, self.rotated())?;
        }
        let mut line = serde_json::to_vec(entry).map_err(io::Error::other)?;
        line.push(b'\n');
        open_for_append(&self.path)?.write_all(&line)
    }

    pub fn read<T: DeserializeOwned>(&self) -> Vec<T> {
        let mut entries = entries_of(&self.rotated());
        entries.extend(entries_of(&self.path));
        entries
    }

    pub fn clear(&self) -> io::Result<()> {
        crate::fsutil::remove_file_if_exists(&self.rotated())?;
        crate::fsutil::remove_file_if_exists(&self.path)
    }
}

pub fn page_of(entries: &[ActivityEntry], before: Option<u64>, size: usize) -> (Vec<ActivityEntry>, Option<u64>) {
    let older: Vec<&ActivityEntry> = entries.iter().filter(|entry| before.is_none_or(|seq| entry.seq < seq)).collect();
    let start = older.len().saturating_sub(size);
    let items: Vec<ActivityEntry> = older[start..].iter().rev().map(|entry| (*entry).clone()).collect();
    let next = items.last().filter(|_| start > 0).map(|entry| entry.seq);
    (items, next)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handled {
    pub agent: AgentId,
    pub name: String,
    pub kind: AgentKind,
    pub project: String,
    pub question: String,
    pub summary: String,
    pub by: By,
    pub outcome: Outcome,
    pub mode: Mode,
}

fn settled_by_octo(entry: &ActivityEntry) -> bool {
    matches!(entry.by, By::Octo | By::Rule) && matches!(entry.outcome, Outcome::Answered | Outcome::Denied)
}

pub struct Activity {
    journal: Journal,
    recent: VecDeque<ActivityEntry>,
    unsaved: VecDeque<ActivityEntry>,
    next_seq: u64,
    offset_ms: i64,
    day: i64,
    today: u32,
    by_project: BTreeMap<String, u32>,
}

impl Activity {
    pub fn new(journal: Journal) -> Self {
        Self {
            journal,
            recent: VecDeque::new(),
            unsaved: VecDeque::new(),
            next_seq: 1,
            offset_ms: 0,
            day: 0,
            today: 0,
            by_project: BTreeMap::new(),
        }
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn load(&mut self, now: u64, offset_ms: i64) {
        let entries: Vec<ActivityEntry> = self.journal.read();
        self.next_seq = entries.iter().map(|entry| entry.seq).max().map_or(1, |last| last + 1);
        self.recent.clear();
        self.start_day(now, offset_ms);
        for entry in entries {
            self.count(&entry);
            push_bounded(&mut self.recent, entry, MAX_ACTIVITY);
        }
    }

    fn start_day(&mut self, now: u64, offset_ms: i64) {
        self.offset_ms = offset_ms;
        self.day = day_of(now, offset_ms);
        self.today = 0;
        self.by_project.clear();
    }

    fn count(&mut self, entry: &ActivityEntry) {
        if day_of(entry.at, self.offset_ms) != self.day || !settled_by_octo(entry) {
            return;
        }
        self.today = self.today.saturating_add(1);
        if self.by_project.len() < MAX_PROJECT_COUNTS || self.by_project.contains_key(&entry.project) {
            let count = self.by_project.entry(entry.project.clone()).or_default();
            *count = count.saturating_add(1);
        }
    }

    pub fn record(&mut self, handled: Handled, now: u64, offset_ms: i64) {
        if day_of(now, offset_ms) != self.day {
            self.start_day(now, offset_ms);
        }
        let entry = ActivityEntry {
            seq: self.next_seq,
            at: now,
            agent: handled.agent,
            name: handled.name,
            kind: handled.kind,
            project: handled.project,
            question: handled.question,
            summary: handled.summary,
            by: handled.by,
            outcome: handled.outcome,
            mode: handled.mode,
        };
        self.next_seq += 1;
        self.count(&entry);
        push_bounded(&mut self.unsaved, entry.clone(), MAX_UNSAVED);
        push_bounded(&mut self.recent, entry, MAX_ACTIVITY);
    }

    pub fn has_unsaved(&self) -> bool {
        !self.unsaved.is_empty()
    }

    pub fn take_unsaved(&mut self) -> Vec<ActivityEntry> {
        self.unsaved.drain(..).collect()
    }

    pub fn handled(&self) -> Vec<ActivityEntry> {
        self.recent.iter().rev().cloned().collect()
    }

    pub fn today(&self, now: u64, offset_ms: i64) -> (u32, Vec<ProjectCount>) {
        if day_of(now, offset_ms) != self.day {
            return (0, Vec::new());
        }
        let projects =
            self.by_project.iter().map(|(path, count)| ProjectCount { path: path.clone(), count: *count }).collect();
        (self.today, projects)
    }
}

pub fn save(journal: &Journal, entries: &[ActivityEntry]) {
    for entry in entries {
        let _ = journal.append(entry);
    }
}

pub fn page(journal: &Journal, before: Option<u64>) -> (Vec<ActivityEntry>, Option<u64>) {
    let mut entries: Vec<ActivityEntry> = journal.read();
    entries.sort_by_key(|entry| entry.seq);
    page_of(&entries, before, ACTIVITY_PAGE)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TalkWrite {
    Append(ChatEntry),
    Clear,
}

pub struct Talk {
    journal: Journal,
    entries: VecDeque<ChatEntry>,
    unsaved: VecDeque<TalkWrite>,
}

impl Talk {
    pub fn new(journal: Journal) -> Self {
        Self { journal, entries: VecDeque::new(), unsaved: VecDeque::new() }
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn load(&mut self) {
        self.entries.clear();
        for entry in self.journal.read() {
            push_bounded(&mut self.entries, entry, MAX_CHAT);
        }
    }

    pub fn say(&mut self, entry: ChatEntry) {
        push_bounded(&mut self.unsaved, TalkWrite::Append(entry.clone()), MAX_UNSAVED);
        push_bounded(&mut self.entries, entry, MAX_CHAT);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> Vec<ChatEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn last(&self, count: usize) -> Vec<ChatEntry> {
        self.entries.iter().skip(self.entries.len().saturating_sub(count)).cloned().collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.unsaved.clear();
        self.unsaved.push_back(TalkWrite::Clear);
    }

    pub fn has_unsaved(&self) -> bool {
        !self.unsaved.is_empty()
    }

    pub fn take_unsaved(&mut self) -> Vec<TalkWrite> {
        self.unsaved.drain(..).collect()
    }
}

pub fn save_talk(journal: &Journal, writes: &[TalkWrite]) {
    for write in writes {
        let _ = match write {
            TalkWrite::Append(entry) => journal.append(entry),
            TalkWrite::Clear => journal.clear(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::ChatRole;
    use crate::util::now_ms;

    const DAY_MS: u64 = crate::util::DAY_MS;

    fn noted(activity: &mut Activity, handled: Handled, now: u64) {
        activity.record(handled, now, 0);
        save(&activity.journal().clone(), &activity.take_unsaved());
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("orkai-octo-logs-{tag}-{}-{}", std::process::id(), now_ms()))
    }

    fn handled(project: &str, by: By, outcome: Outcome) -> Handled {
        Handled {
            agent: 1,
            name: "api".into(),
            kind: AgentKind::Claude,
            project: project.into(),
            question: "Run the tests?".into(),
            summary: "Allowed npm test".into(),
            by,
            outcome,
            mode: Mode::Moderate,
        }
    }

    fn entry(seq: u64) -> ActivityEntry {
        ActivityEntry {
            seq,
            at: seq,
            agent: 1,
            name: "api".into(),
            kind: AgentKind::Claude,
            project: "p".into(),
            question: String::new(),
            summary: format!("entry {seq}"),
            by: By::Octo,
            outcome: Outcome::Answered,
            mode: Mode::Bypass,
        }
    }

    #[test]
    fn a_bounded_list_drops_its_oldest_item() {
        let mut items = VecDeque::new();
        for item in 1..=5 {
            push_bounded(&mut items, item, 3);
        }
        assert_eq!(items, [3, 4, 5]);
        push_bounded(&mut items, 6, 0);
        assert_eq!(items, [6], "a limit of nothing still keeps the newest item");
    }

    #[test]
    fn a_journal_appends_lines_creates_its_folder_and_skips_what_it_cannot_read() {
        let dir = temp_dir("journal");
        let journal = Journal::new(dir.join("nested").join("activity.jsonl"));
        assert!(journal.read::<ActivityEntry>().is_empty(), "a journal never written reads as empty");
        journal.append(&entry(1)).unwrap();
        journal.append(&entry(2)).unwrap();
        let mut file = OpenOptions::new().append(true).open(journal.path()).unwrap();
        file.write_all(b"{ broken\n\n{\"seq\":\"wrong\"}\n").unwrap();
        journal.append(&entry(3)).unwrap();
        assert_eq!(journal.read::<ActivityEntry>(), [entry(1), entry(2), entry(3)]);
        assert_eq!(fs::read_to_string(journal.path()).unwrap().lines().count(), 6);
        journal.clear().unwrap();
        assert!(!journal.path().exists());
        journal.clear().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_journal_rotates_at_one_mebibyte_and_keeps_one_older_file() {
        let dir = temp_dir("rotate");
        let journal = Journal::new(dir.join("activity.jsonl"));
        assert_eq!(journal.rotated(), dir.join("activity.old.jsonl"));
        journal.append(&entry(1)).unwrap();
        let mut file = OpenOptions::new().append(true).open(journal.path()).unwrap();
        file.write_all(&vec![b' '; usize::try_from(MAX_LOG_BYTES).unwrap()]).unwrap();
        file.write_all(b"\n").unwrap();
        drop(file);
        journal.append(&entry(2)).unwrap();
        assert!(journal.rotated().exists(), "the full file is set aside");
        assert!(fs::metadata(journal.path()).unwrap().len() < 1024, "and a new one starts");
        assert_eq!(journal.read::<ActivityEntry>(), [entry(1), entry(2)], "both are still read, oldest first");

        fs::write(journal.path(), vec![b' '; usize::try_from(MAX_LOG_BYTES).unwrap()]).unwrap();
        journal.append(&entry(3)).unwrap();
        assert_eq!(journal.read::<ActivityEntry>(), [entry(3)], "only one older file is ever kept");
        let files = fs::read_dir(&dir).unwrap().count();
        assert_eq!(files, 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_activity_is_read_in_pages_from_the_newest_to_the_oldest() {
        let entries: Vec<ActivityEntry> = (1..=7).map(entry).collect();
        let seqs = |items: &[ActivityEntry]| items.iter().map(|item| item.seq).collect::<Vec<_>>();
        let (first, next) = page_of(&entries, None, 3);
        assert_eq!((seqs(&first), next), (vec![7, 6, 5], Some(5)));
        let (second, next) = page_of(&entries, next, 3);
        assert_eq!((seqs(&second), next), (vec![4, 3, 2], Some(2)));
        let (last, next) = page_of(&entries, next, 3);
        assert_eq!((seqs(&last), next), (vec![1], None), "the last page says nothing is left");
        assert_eq!(page_of(&entries, Some(1), 3), (Vec::new(), None));
        assert_eq!(page_of(&[], None, 3), (Vec::new(), None));
        let (exact, next) = page_of(&entries, Some(4), 3);
        assert_eq!((seqs(&exact), next), (vec![3, 2, 1], None));
    }

    #[test]
    fn the_activity_keeps_fifty_entries_in_memory_and_every_entry_on_disk() {
        let dir = temp_dir("activity");
        let mut activity = Activity::new(Journal::new(dir.join("activity.jsonl")));
        activity.load(1_000, 0);
        for _ in 0..MAX_ACTIVITY + 5 {
            noted(&mut activity, handled("p", By::Octo, Outcome::Answered), 1_000);
        }
        let shown = activity.handled();
        assert_eq!(shown.len(), MAX_ACTIVITY);
        assert_eq!((shown[0].seq, shown[MAX_ACTIVITY - 1].seq), (55, 6), "newest first, the oldest dropped");
        let (first, next) = page(activity.journal(), None);
        assert_eq!((first.len(), first[0].seq, next), (ACTIVITY_PAGE, 55, Some(6)));
        let (rest, next) = page(activity.journal(), next);
        assert_eq!((rest.iter().map(|item| item.seq).collect::<Vec<_>>(), next), (vec![5, 4, 3, 2, 1], None));

        let mut reopened = Activity::new(Journal::new(dir.join("activity.jsonl")));
        reopened.load(1_000, 0);
        assert_eq!(reopened.handled(), shown, "the next run starts from what was logged");
        noted(&mut reopened, handled("p", By::You, Outcome::Answered), 1_000);
        assert_eq!(reopened.handled()[0].seq, 56, "and keeps numbering after it");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn today_counts_only_what_octo_settled_per_project_and_starts_over_each_day() {
        let dir = temp_dir("today");
        let mut activity = Activity::new(Journal::new(dir.join("activity.jsonl")));
        let morning = 3 * DAY_MS + 5;
        activity.load(morning, 0);
        noted(&mut activity, handled("a", By::Octo, Outcome::Answered), morning);
        noted(&mut activity, handled("a", By::Rule, Outcome::Answered), morning);
        noted(&mut activity, handled("b", By::Octo, Outcome::Denied), morning);
        noted(&mut activity, handled("b", By::Octo, Outcome::Escalated), morning);
        noted(&mut activity, handled("b", By::Octo, Outcome::Failed), morning);
        noted(&mut activity, handled("b", By::You, Outcome::Answered), morning);
        let count = |path: &str, count| ProjectCount { path: path.into(), count };
        assert_eq!(activity.today(morning, 0), (3, vec![count("a", 2), count("b", 1)]));

        let mut reopened = Activity::new(Journal::new(dir.join("activity.jsonl")));
        reopened.load(morning + 1, 0);
        assert_eq!(
            reopened.today(morning + 1, 0),
            (3, vec![count("a", 2), count("b", 1)]),
            "counted again from the log"
        );

        let tomorrow = 4 * DAY_MS + 1;
        assert_eq!(activity.today(tomorrow, 0), (0, Vec::new()), "a new day shows nothing before anything happens");
        noted(&mut activity, handled("a", By::Octo, Outcome::Answered), tomorrow);
        assert_eq!(activity.today(tomorrow, 0), (1, vec![count("a", 1)]));
        assert_eq!(activity.handled().len(), 7, "the list still shows yesterday");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn today_is_the_day_of_the_computer_and_entries_wait_in_order_until_they_are_saved() {
        let dir = temp_dir("local-day");
        let mut activity = Activity::new(Journal::new(dir.join("activity.jsonl")));
        let west = -3 * 3_600_000;
        let late_evening = 3 * DAY_MS + 3_600_000;
        activity.load(late_evening, west);
        activity.record(handled("a", By::Octo, Outcome::Answered), late_evening - 2 * 3_600_000, west);
        activity.record(handled("a", By::Octo, Outcome::Answered), late_evening, west);
        assert_eq!(activity.today(late_evening, west).0, 2, "midnight in Greenwich does not split the evening");
        let after_local_midnight = 3 * DAY_MS + 4 * 3_600_000;
        assert_eq!(activity.today(after_local_midnight, west).0, 0, "the count starts over at midnight at home");
        assert!(activity.journal().read::<ActivityEntry>().is_empty(), "recording touches no file");
        assert_eq!(activity.handled().len(), 2, "and the entries show at once");
        let waiting = activity.take_unsaved();
        assert_eq!(waiting.iter().map(|entry| entry.seq).collect::<Vec<_>>(), [1, 2]);
        assert!(activity.take_unsaved().is_empty());
        save(activity.journal(), &waiting);
        assert_eq!(activity.journal().read::<ActivityEntry>(), waiting);

        let mut reopened = Activity::new(Journal::new(dir.join("activity.jsonl")));
        reopened.load(late_evening, west);
        assert_eq!(reopened.today(late_evening, west).0, 2);
        reopened.load(late_evening, 0);
        assert_eq!(reopened.today(late_evening, 0).0, 1, "a computer in Greenwich would count one of them today");
        reopened.load(after_local_midnight, west);
        assert_eq!(reopened.today(after_local_midnight, west).0, 0);
        for _ in 0..MAX_UNSAVED + 3 {
            reopened.record(handled("a", By::You, Outcome::Answered), after_local_midnight, west);
        }
        assert_eq!(reopened.take_unsaved().len(), MAX_UNSAVED, "what waits to be saved is bounded");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_projects_counted_today_are_bounded() {
        let dir = temp_dir("projects");
        let mut activity = Activity::new(Journal::new(dir.join("activity.jsonl")));
        activity.load(0, 0);
        for index in 0..MAX_PROJECT_COUNTS + 3 {
            noted(&mut activity, handled(&format!("p{index:03}"), By::Octo, Outcome::Answered), 0);
        }
        noted(&mut activity, handled("p000", By::Octo, Outcome::Answered), 0);
        let (total, projects) = activity.today(0, 0);
        assert_eq!(total, u32::try_from(MAX_PROJECT_COUNTS + 4).unwrap());
        assert_eq!(projects.len(), MAX_PROJECT_COUNTS);
        assert_eq!(projects[0], ProjectCount { path: "p000".into(), count: 2 });
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_chat_keeps_sixty_entries_in_memory_survives_a_restart_and_can_be_cleared() {
        let dir = temp_dir("chat");
        let said =
            |at: u64| ChatEntry { at, role: ChatRole::You, text: format!("m{at}"), actions: Vec::new(), failed: false };
        let saved = |talk: &mut Talk| save_talk(&talk.journal().clone(), &talk.take_unsaved());
        let mut talk = Talk::new(Journal::new(dir.join("chat.jsonl")));
        talk.load();
        for at in 0..u64::try_from(MAX_CHAT).unwrap() + 4 {
            talk.say(said(at));
        }
        assert!(!talk.journal().path().exists(), "saying something touches no file");
        assert!(talk.has_unsaved());
        saved(&mut talk);
        assert!(!talk.has_unsaved());
        let entries = talk.entries();
        assert_eq!(entries.len(), MAX_CHAT);
        assert_eq!((entries[0].at, entries[MAX_CHAT - 1].at), (4, 63), "oldest first, the oldest dropped");
        assert_eq!(talk.last(2).iter().map(|entry| entry.at).collect::<Vec<_>>(), [62, 63]);
        assert_eq!(talk.last(500).len(), MAX_CHAT);

        let mut reopened = Talk::new(Journal::new(dir.join("chat.jsonl")));
        reopened.load();
        assert_eq!(reopened.entries(), entries);
        reopened.say(said(99));
        reopened.clear();
        reopened.say(said(100));
        assert_eq!(reopened.entries(), [said(100)]);
        assert_eq!(
            reopened.take_unsaved(),
            [TalkWrite::Clear, TalkWrite::Append(said(100))],
            "what was said before a clear is not written after it"
        );
        save_talk(reopened.journal(), &[TalkWrite::Clear, TalkWrite::Append(said(100))]);
        let mut after = Talk::new(Journal::new(dir.join("chat.jsonl")));
        after.load();
        assert_eq!(after.entries(), [said(100)], "a cleared chat stays cleared and keeps what came after");
        after.clear();
        saved(&mut after);
        assert!(!after.journal().path().exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
