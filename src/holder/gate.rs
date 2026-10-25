use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::launch;
use crate::model::{AgentId, AgentState, TestReport, TestState, TestTarget};
use crate::paths::Home;
use crate::project;
use crate::tui::format::duration_short;
use crate::util::{lock, now_ms};

pub const TEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const POLL: Duration = Duration::from_millis(200);

#[derive(Default)]
struct Run {
    report: Option<TestReport>,
    pid: Option<u32>,
    generation: u64,
}

pub struct Gate {
    home: Home,
    id: AgentId,
    target: TestTarget,
    timeout: Duration,
    run: Mutex<Run>,
}

pub fn turn_ended(before: Option<AgentState>, now: AgentState) -> bool {
    now == AgentState::Done && before != Some(AgentState::Done)
}

impl Gate {
    pub fn new(home: Home, id: AgentId, target: TestTarget) -> Self {
        Self::with_timeout(home, id, target, TEST_TIMEOUT)
    }

    pub fn with_timeout(home: Home, id: AgentId, target: TestTarget, timeout: Duration) -> Self {
        Self { home, id, target, timeout, run: Mutex::new(Run::default()) }
    }

    pub fn report(&self) -> Option<TestReport> {
        lock(&self.run).report.clone()
    }

    pub fn start(self: &Arc<Self>) {
        let mut run = lock(&self.run);
        if let Some(pid) = run.pid.take() {
            kill(pid);
        }
        run.generation += 1;
        let loaded = match project::load(&self.target.repo) {
            Ok(Some(loaded)) => loaded,
            Ok(None) => {
                run.report = None;
                return;
            }
            Err(err) => {
                let _ = std::fs::write(self.home.test_log(self.id), format!("{err}\n"));
                run.report = Some(finished(TestState::Failed, String::new(), now_ms(), None));
                return;
            }
        };
        let Some(command) = loaded.config.test_command().map(str::to_string) else {
            run.report = None;
            return;
        };
        let started = now_ms();
        if !project::is_trusted(&self.home, &loaded) {
            run.report = Some(TestReport {
                state: TestState::Untrusted,
                command,
                started_at_ms: started,
                finished_at_ms: None,
                code: None,
            });
            return;
        }
        let log = self.home.test_log(self.id);
        let child = match spawn(&command, &self.target.dir, &log) {
            Ok(child) => child,
            Err(err) => {
                let _ = std::fs::write(&log, format!("could not run {command}: {err}\n"));
                run.report = Some(finished(TestState::Failed, command, started, None));
                return;
            }
        };
        run.pid = Some(child.id());
        run.report = Some(TestReport {
            state: TestState::Running,
            command: command.clone(),
            started_at_ms: started,
            finished_at_ms: None,
            code: None,
        });
        let generation = run.generation;
        drop(run);
        let gate = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("orkai-test".into())
            .spawn(move || gate.wait(child, generation, command, started));
    }

    fn wait(&self, mut child: Child, generation: u64, command: String, started: u64) {
        let deadline = Instant::now() + self.timeout;
        let code = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) if Instant::now() >= deadline => {
                    kill(child.id());
                    let _ = child.wait();
                    if let Ok(mut log) = File::options().append(true).open(self.home.test_log(self.id)) {
                        let limit = duration_short(u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX));
                        let _ = writeln!(log, "\norkai stopped {command} after {limit}");
                    }
                    break None;
                }
                Ok(None) => thread::sleep(POLL),
                Err(_) => break None,
            }
        };
        let mut run = lock(&self.run);
        if run.generation != generation {
            return;
        }
        run.pid = None;
        let state = if code == Some(0) { TestState::Passed } else { TestState::Failed };
        run.report = Some(finished(state, command, started, code));
    }

    pub fn stop(&self) {
        if let Some(pid) = lock(&self.run).pid.take() {
            kill(pid);
        }
    }
}

fn finished(state: TestState, command: String, started: u64, code: Option<i32>) -> TestReport {
    TestReport { state, command, started_at_ms: started, finished_at_ms: Some(now_ms()), code }
}

fn spawn(command: &str, dir: &Path, log: &Path) -> io::Result<Child> {
    let out = File::create(log)?;
    let err = out.try_clone()?;
    let mut cmd = shell_command(command);
    cmd.current_dir(dir).stdin(Stdio::null()).stdout(out).stderr(err);
    launch::without_console(&mut cmd).spawn()
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let shell = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    let mut cmd = Command::new(shell);
    cmd.raw_arg("/D /S /C").raw_arg(format!("\"{command}\""));
    cmd
}

#[cfg(unix)]
fn shell_command(command: &str) -> Command {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command).process_group(0);
    cmd
}

fn kill(pid: u32) {
    #[cfg(unix)]
    if let Ok(pid) = libc::pid_t::try_from(pid) {
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    launch::kill_process_tree(pid);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        dir: PathBuf,
        home: Home,
        repo: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture() -> Fixture {
        let dir = std::env::temp_dir().join(format!(
            "orkai-gate-{}-{}-{}",
            std::process::id(),
            now_ms(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let home = Home::at(dir.join("home"));
        home.ensure().unwrap();
        std::fs::create_dir_all(home.agent_dir(1)).unwrap();
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        Fixture { dir, home, repo }
    }

    fn new_gate(fx: &Fixture, timeout: Duration) -> Arc<Gate> {
        let target = TestTarget { repo: fx.repo.clone(), dir: fx.repo.clone() };
        Arc::new(Gate::with_timeout(fx.home.clone(), 1, target, timeout))
    }

    fn configure(fx: &Fixture, command: &str, trusted: bool) {
        let text = serde_json::json!({ "test": command }).to_string();
        std::fs::write(fx.repo.join(project::FILE), text).unwrap();
        if trusted {
            project::trust(&fx.home, &project::load(&fx.repo).unwrap().unwrap()).unwrap();
        }
    }

    fn settled(gate: &Gate) -> TestReport {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let report = gate.report().unwrap();
            if report.state != TestState::Running {
                return report;
            }
            assert!(Instant::now() < deadline, "the test run never finished");
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn long_command() -> &'static str {
        if cfg!(windows) { "ping -n 60 127.0.0.1 >NUL" } else { "sleep 60" }
    }

    #[test]
    fn only_a_finished_turn_starts_the_tests() {
        assert!(turn_ended(Some(AgentState::Working), AgentState::Done));
        assert!(turn_ended(None, AgentState::Done));
        assert!(!turn_ended(Some(AgentState::Done), AgentState::Done));
        assert!(!turn_ended(Some(AgentState::Working), AgentState::Idle));
        assert!(!turn_ended(Some(AgentState::Done), AgentState::Idle));
    }

    #[test]
    fn a_trusted_command_runs_in_the_worktree_and_reports_its_exit_code() {
        let fx = fixture();
        let gate = new_gate(&fx, TEST_TIMEOUT);
        configure(&fx, "echo orkai-gate-output && exit 3", true);
        gate.start();
        let failed = settled(&gate);
        assert_eq!((failed.state, failed.code), (TestState::Failed, Some(3)));
        assert_eq!(failed.command, "echo orkai-gate-output && exit 3");
        assert!(failed.finished_at_ms.is_some());
        let log = std::fs::read_to_string(fx.home.test_log(1)).unwrap();
        assert!(log.contains("orkai-gate-output"), "{log}");

        configure(&fx, "exit 0", true);
        gate.start();
        let passed = settled(&gate);
        assert_eq!((passed.state, passed.code), (TestState::Passed, Some(0)));
    }

    #[test]
    fn nothing_runs_without_a_trusted_test_command() {
        let fx = fixture();
        let gate = new_gate(&fx, TEST_TIMEOUT);
        gate.start();
        assert_eq!(gate.report(), None, "no .orkai.json");

        configure(&fx, "exit 0", false);
        gate.start();
        let report = gate.report().unwrap();
        assert_eq!((report.state, report.command.as_str()), (TestState::Untrusted, "exit 0"));
        assert!(!fx.home.test_log(1).exists(), "an untrusted command never runs");

        std::fs::write(fx.repo.join(project::FILE), r#"{"copy":[".env"]}"#).unwrap();
        gate.start();
        assert_eq!(gate.report(), None, "no test command");
    }

    #[test]
    fn a_new_turn_replaces_the_run_in_progress_and_a_slow_run_times_out() {
        let fx = fixture();
        let gate = new_gate(&fx, TEST_TIMEOUT);
        configure(&fx, long_command(), true);
        gate.start();
        assert_eq!(gate.report().unwrap().state, TestState::Running);
        configure(&fx, "exit 0", true);
        let replaced = Instant::now();
        gate.start();
        assert_eq!(settled(&gate).state, TestState::Passed, "the old run must not overwrite the new one");
        assert!(replaced.elapsed() < Duration::from_secs(20));

        let slow = new_gate(&fx, Duration::from_millis(500));
        configure(&fx, long_command(), true);
        slow.start();
        let report = settled(&slow);
        assert_eq!((report.state, report.code), (TestState::Failed, None));
        let log = std::fs::read_to_string(fx.home.test_log(1)).unwrap();
        assert!(log.contains("orkai stopped"), "{log}");

        configure(&fx, long_command(), true);
        gate.start();
        gate.stop();
        assert_eq!(settled(&gate).state, TestState::Failed, "a stopped run ends as failed");
    }
}
