/// Tasks sent to agents while this orkai runs, recalled with ctrl+↑.
///
/// The history lives in memory only: closing orkai forgets it.
#[derive(Default)]
pub struct TaskHistory {
    tasks: Vec<String>,
    position: Option<usize>,
    draft: Option<String>,
}

impl TaskHistory {
    pub fn record(&mut self, task: &str) {
        let task = task.trim();
        if task.is_empty() {
            return;
        }
        self.position = None;
        self.draft = None;
        if self.tasks.last().is_some_and(|last| last == task) {
            return;
        }
        self.tasks.push(task.to_string());
    }

    /// The task before the current one, remembering the text being written.
    pub fn older(&mut self, draft: &str) -> Option<String> {
        if self.tasks.is_empty() {
            return None;
        }
        let index = match self.position {
            Some(position) => position.saturating_sub(1),
            None => {
                self.draft = Some(draft.to_string());
                self.tasks.len() - 1
            }
        };
        self.position = Some(index);
        Some(self.tasks[index].clone())
    }

    /// The task after the current one, or the text that was being written.
    pub fn newer(&mut self) -> Option<String> {
        let position = self.position?;
        let next = position + 1;
        if next < self.tasks.len() {
            self.position = Some(next);
            return Some(self.tasks[next].clone());
        }
        self.position = None;
        Some(self.draft.take().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_recalled_before_the_first_task() {
        let mut history = TaskHistory::default();
        assert_eq!(history.older("draft"), None);
        assert_eq!(history.newer(), None);
    }

    #[test]
    fn older_walks_back_through_the_tasks() {
        let mut history = TaskHistory::default();
        history.record("first");
        history.record("second");
        assert_eq!(history.older("draft"), Some("second".to_string()));
        assert_eq!(history.older(""), Some("first".to_string()));
        assert_eq!(history.older(""), Some("first".to_string()));
    }

    #[test]
    fn newer_comes_back_and_restores_the_draft() {
        let mut history = TaskHistory::default();
        history.record("first");
        history.record("second");
        assert_eq!(history.older("half typed"), Some("second".to_string()));
        assert_eq!(history.newer(), Some("half typed".to_string()));
        assert_eq!(history.newer(), None);
    }

    #[test]
    fn the_same_task_twice_is_kept_once() {
        let mut history = TaskHistory::default();
        history.record("fix the bug");
        history.record("fix the bug");
        assert_eq!(history.older(""), Some("fix the bug".to_string()));
        assert_eq!(history.older(""), Some("fix the bug".to_string()));
    }

    #[test]
    fn empty_tasks_are_ignored() {
        let mut history = TaskHistory::default();
        history.record("   ");
        assert_eq!(history.older(""), None);
    }
}
