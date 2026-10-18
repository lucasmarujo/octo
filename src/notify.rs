use std::io;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::launch;
use crate::model::{AgentId, AgentKind, AgentState};

pub const QUEUE: usize = 16;
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const WAIT_STEP: Duration = Duration::from_millis(50);
const BELL: u8 = 0x07;
const NOTIFICATION_TITLE: &str = "orkai";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub id: AgentId,
    pub name: String,
    pub kind: AgentKind,
    pub state: AgentState,
    pub session: String,
    pub cwd: PathBuf,
    pub activity: String,
}

impl Alert {
    pub fn message(&self) -> String {
        match self.state {
            AgentState::Paused => format!("{} is waiting for you", self.name),
            _ => format!("{} finished", self.name),
        }
    }

    pub fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ORKAI_AGENT_ID", self.id.to_string()),
            ("ORKAI_AGENT_NAME", self.name.clone()),
            ("ORKAI_AGENT_KIND", self.kind.label().to_string()),
            ("ORKAI_AGENT_STATE", self.state.label().to_string()),
            ("ORKAI_SESSION", self.session.clone()),
            ("ORKAI_AGENT_CWD", self.cwd.to_string_lossy().into_owned()),
            ("ORKAI_AGENT_ACTIVITY", self.activity.clone()),
            ("ORKAI_MESSAGE", self.message()),
        ]
    }
}

fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

pub fn terminal_sequence(alerts: &[Alert]) -> Vec<u8> {
    let body = match alerts {
        [] => return Vec::new(),
        [alert] => alert.message(),
        many => format!("{} agents need you", many.len()),
    };
    let mut out = vec![BELL];
    out.extend_from_slice(format!("\x1b]9;{NOTIFICATION_TITLE}: {}\x07", printable(&body)).as_bytes());
    out
}

pub struct Notifier {
    tx: Option<SyncSender<Alert>>,
}

impl Notifier {
    pub fn start(config: &Config) -> Self {
        let Some(command) = config.notify_command.clone().filter(|c| !c.trim().is_empty()) else {
            return Self { tx: None };
        };
        let args = config.notify_args.clone();
        let (tx, rx) = mpsc::sync_channel::<Alert>(QUEUE);
        let spawned = thread::Builder::new().name("orkai-notify".into()).spawn(move || {
            for alert in rx {
                let _ = run_command(&command, &args, &alert, COMMAND_TIMEOUT);
            }
        });
        Self { tx: spawned.ok().map(|_| tx) }
    }

    pub fn send(&self, alert: Alert) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(alert);
        }
    }
}

pub fn run_command(command: &str, args: &[String], alert: &Alert, timeout: Duration) -> io::Result<ExitStatus> {
    let program = launch::resolve_program(command)?;
    let (program, args) = launch::wrap_for_platform(program, args.to_vec())?;
    let mut cmd = Command::new(program);
    cmd.args(args).envs(alert.env()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = launch::spawn_detached(&mut cmd)?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            launch::kill_process_tree(child.id());
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, format!("{command} did not finish in time")));
        }
        thread::sleep(WAIT_STEP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::now_ms;

    fn alert(name: &str, state: AgentState) -> Alert {
        Alert {
            id: 7,
            name: name.into(),
            kind: AgentKind::Claude,
            state,
            session: "default".into(),
            cwd: PathBuf::from("/work/app"),
            activity: "Allow Bash: npm test".into(),
        }
    }

    #[test]
    fn messages_say_what_the_agent_needs() {
        assert_eq!(alert("fix-login", AgentState::Paused).message(), "fix-login is waiting for you");
        assert_eq!(alert("fix-login", AgentState::Done).message(), "fix-login finished");
    }

    #[test]
    fn terminal_sequence_rings_once_and_sends_one_notification() {
        assert!(terminal_sequence(&[]).is_empty());
        let one = terminal_sequence(&[alert("fix-login", AgentState::Paused)]);
        assert_eq!(one, b"\x07\x1b]9;orkai: fix-login is waiting for you\x07");
        let many = terminal_sequence(&[alert("a", AgentState::Paused), alert("b", AgentState::Done)]);
        assert_eq!(many, b"\x07\x1b]9;orkai: 2 agents need you\x07");
        let hostile = terminal_sequence(&[alert("x\x07\x1b]0;pwned\u{9c}", AgentState::Done)]);
        assert_eq!(hostile, b"\x07\x1b]9;orkai: x]0;pwned finished\x07");
    }

    #[test]
    fn command_environment_describes_the_agent() {
        let env = alert("fix-login", AgentState::Paused).env();
        let get = |key: &str| env.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str());
        assert_eq!(get("ORKAI_AGENT_ID"), Some("7"));
        assert_eq!(get("ORKAI_AGENT_NAME"), Some("fix-login"));
        assert_eq!(get("ORKAI_AGENT_KIND"), Some("claude"));
        assert_eq!(get("ORKAI_AGENT_STATE"), Some("paused"));
        assert_eq!(get("ORKAI_SESSION"), Some("default"));
        assert_eq!(get("ORKAI_AGENT_ACTIVITY"), Some("Allow Bash: npm test"));
        assert_eq!(get("ORKAI_MESSAGE"), Some("fix-login is waiting for you"));
    }

    fn script_writing(dir: &std::path::Path, target: &std::path::Path) -> PathBuf {
        if cfg!(windows) {
            let script = dir.join("notify.cmd");
            let body = format!("@echo %ORKAI_MESSAGE%^|%ORKAI_AGENT_STATE%> \"{}\"\r\n", target.display());
            std::fs::write(&script, body).unwrap();
            script
        } else {
            let script = dir.join("notify.sh");
            let body = format!(
                "#!/bin/sh\nprintf '%s|%s' \"$ORKAI_MESSAGE\" \"$ORKAI_AGENT_STATE\" > '{}'\n",
                target.display()
            );
            std::fs::write(&script, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            script
        }
    }

    #[test]
    fn the_configured_command_runs_with_the_alert_in_its_environment() {
        let dir = std::env::temp_dir().join(format!("orkai-notify-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("alert.txt");
        let command = script_writing(&dir, &target).to_string_lossy().into_owned();
        let status = run_command(&command, &[], &alert("fix-login", AgentState::Paused), COMMAND_TIMEOUT).unwrap();
        assert!(status.success());
        let written = std::fs::read_to_string(&target).unwrap();
        assert_eq!(written.trim(), "fix-login is waiting for you|paused");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_command_that_hangs_is_stopped_after_the_timeout() {
        let (command, args): (&str, Vec<String>) = if cfg!(windows) {
            ("ping.exe", vec!["-n".into(), "30".into(), "127.0.0.1".into()])
        } else {
            ("sleep", vec!["30".into()])
        };
        let started = Instant::now();
        let err = run_command(command, &args, &alert("x", AgentState::Done), Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_missing_command_is_reported() {
        let err =
            run_command("orkai-missing-notifier", &[], &alert("x", AgentState::Done), COMMAND_TIMEOUT).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn without_a_command_alerts_are_dropped() {
        let notifier = Notifier::start(&Config::default());
        assert!(notifier.tx.is_none());
        notifier.send(alert("x", AgentState::Done));
    }
}
