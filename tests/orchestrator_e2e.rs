use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use orkai::client;
use orkai::config::Config;
use orkai::launch::Launcher;
use orkai::model::{AgentId, AgentKind, AgentState};
use orkai::orchestrator::{
    ActivityEntry, By, CliEngine, DecisionStatus, Mode, OrchestratorView, Outcome, SettingsChange,
};
use orkai::paths::Home;
use orkai::protocol::ClientMsg;
use orkai::pwa::hub::Hub;
use orkai::pwa::protocol::{BridgeMsg, PhoneMsg, StartRequest};
use orkai::pwa::server::{Outbox, Peer};
use orkai::store::Store;

const TIMEOUT: Duration = Duration::from_secs(90);
const HOST: &str = "testhost";
const QUESTION: &str = "Do you want to run the tests? (y/n)";
const ANSWER_FILE: &str = "answer.txt";
const PROMPT_FILE: &str = "prompt.txt";
const VERDICT: &str = r#"{\"action\":\"answer\",\"keys\":null,\"text\":\"yes\",\"summary\":\"Told it to run the tests\",\"reason\":\"the task asks for them\",\"tag\":null,\"deny\":false}"#;
static COUNTER: AtomicU64 = AtomicU64::new(0);

const FAKE_PS1_HEAD: &str = r#"
Add-Type -Namespace Orkai -Name Console -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern IntPtr GetStdHandle(int handle);
[DllImport("kernel32.dll")] public static extern bool SetConsoleMode(IntPtr handle, int mode);
'@
[void][Orkai.Console]::SetConsoleMode([Orkai.Console]::GetStdHandle(-10), 0x200)
[void][Orkai.Console]::SetConsoleMode([Orkai.Console]::GetStdHandle(-11), 0x7)
$e = [char]27
"#;

struct Fixture {
    dir: PathBuf,
    store: Store,
    hub: Arc<Hub>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for record in self.store.list_agents().unwrap_or_default() {
            if let Ok(Some(runtime)) = self.store.read_runtime(record.id) {
                let _ = client::deliver(&runtime, &ClientMsg::Kill);
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn script(dir: &Path, name: &str, ps1: &str, sh: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        let script = dir.join(format!("{name}.ps1"));
        std::fs::write(&script, ps1).unwrap();
        let args = ["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"].map(String::from);
        ("powershell.exe".to_string(), [args.to_vec(), vec![script.to_string_lossy().into_owned()]].concat())
    } else {
        let script = dir.join(format!("{name}.sh"));
        std::fs::write(&script, sh).unwrap();
        ("/bin/sh".to_string(), vec![script.to_string_lossy().into_owned()])
    }
}

fn asking_agent(dir: &Path) -> (String, Vec<String>) {
    let ps1 = format!(
        "{FAKE_PS1_HEAD}[Console]::Write(\"$e[2J$e[999;1H{QUESTION}\")
$line = ''
while (($c = [char][Console]::In.Read()) -ne [char]13) {{ $line += $c }}
[IO.File]::WriteAllText(\"$PWD\\{ANSWER_FILE}\", $line)
[Console]::Write(\"$e[2J$e[999;1Hthanks\")
Start-Sleep -Seconds 120
"
    );
    let sh = format!(
        "printf '\\033[2J\\033[999;1H{QUESTION}'
read orkai_line
printf '%s' \"$orkai_line\" > {ANSWER_FILE}
printf '\\033[2J\\033[999;1Hthanks'
sleep 120
"
    );
    script(dir, "agent", &ps1, &sh)
}

fn judging_engine(dir: &Path) -> (String, Vec<String>) {
    let ps1 = format!(
        "$prompt = [Console]::In.ReadToEnd()
[IO.File]::WriteAllText(\"$PSScriptRoot\\{PROMPT_FILE}\", $prompt)
[Console]::Out.Write('{{\"type\":\"result\",\"is_error\":false,\"result\":\"{VERDICT}\"}}')
"
    );
    let sh = format!(
        "cat > \"$(dirname \"$0\")/{PROMPT_FILE}\"
printf '%s' '{{\"type\":\"result\",\"is_error\":false,\"result\":\"{VERDICT}\"}}'
"
    );
    script(dir, "engine", &ps1, &sh)
}

fn fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "orkai-octo-e2e-{}-{}-{}",
        std::process::id(),
        orkai::util::now_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let home = Home::at(dir.join("home"));
    let store = Store::open(home.clone()).unwrap();
    let (kimi_command, kimi_args) = asking_agent(&dir);
    let (claude_command, claude_args) = judging_engine(&dir);
    let config = Config { kimi_command, kimi_args, claude_command, claude_args, ..Config::default() };
    let launcher = Launcher::new(store.clone(), config, Path::new(env!("CARGO_BIN_EXE_orkai"))).unwrap();
    let hub = Hub::orchestrating(launcher, HOST.to_string(), Arc::new(CliEngine::new(&home)));
    Fixture { dir, store, hub }
}

struct Window(Vec<BridgeMsg>);

impl Outbox for Window {
    fn deliver(&mut self, plain: &[u8]) -> std::io::Result<()> {
        self.0.push(serde_json::from_slice(plain).map_err(std::io::Error::other)?);
        Ok(())
    }
}

fn pump_until<T>(peer: &mut Peer, window: &mut Window, wanted: impl Fn(&BridgeMsg) -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        peer.wait(window).unwrap();
        if let Some(found) = window.0.iter().find_map(&wanted) {
            return found;
        }
    }
    let seen: Vec<String> = window.0.iter().filter_map(|message| serde_json::to_string(message).ok()).collect();
    panic!("the message did not arrive in time; the window saw {seen:#?}");
}

fn view_where(peer: &mut Peer, window: &mut Window, check: impl Fn(&OrchestratorView) -> bool) -> OrchestratorView {
    pump_until(peer, window, |message| match message {
        BridgeMsg::Orchestrator(view) if check(view) => Some(view.clone()),
        _ => None,
    })
}

fn state_of(peer: &mut Peer, window: &mut Window, dir: &str, state: AgentState) -> AgentId {
    pump_until(peer, window, |message| match message {
        BridgeMsg::State(overview) => {
            overview.agents.iter().find(|agent| agent.cwd.ends_with(dir) && agent.state == state).map(|agent| agent.id)
        }
        _ => None,
    })
}

#[test]
fn in_moderate_mode_octo_answers_a_paused_agent_through_the_engine_cli_and_logs_it() {
    let fx = fixture();
    let mut peer = Peer::join(&fx.hub);
    let mut window = Window(Vec::new());
    let enable = SettingsChange {
        enabled: Some(true),
        mode: Some(Mode::Moderate),
        primary: Some(Some(AgentKind::Claude)),
        ..SettingsChange::default()
    };
    peer.handle(PhoneMsg::OrchestratorSet(enable), &mut window).unwrap();
    let enabled = view_where(&mut peer, &mut window, |view| view.enabled);
    assert_eq!((enabled.mode, enabled.primary), (Mode::Moderate, Some(AgentKind::Claude)));
    assert!(enabled.engines.iter().any(|engine| engine.kind == AgentKind::Claude && engine.installed));

    let cwd = fx.dir.join("asks");
    std::fs::create_dir_all(&cwd).unwrap();
    let start = PhoneMsg::Start(StartRequest {
        kind: AgentKind::Kimi,
        session: "default".into(),
        cwd: cwd.to_string_lossy().into_owned(),
        task: String::new(),
        worktree: false,
        bypass: false,
        remote_control: false,
        model: None,
        attachments: Vec::new(),
    });
    peer.handle(start, &mut window).unwrap();
    let agent = state_of(&mut peer, &mut window, "asks", AgentState::Paused);

    let deciding = view_where(&mut peer, &mut window, |view| {
        view.decisions.iter().any(|decision| decision.agent == agent && !decision.question.is_empty())
    });
    let decision = deciding.decisions.iter().find(|decision| decision.agent == agent).unwrap();
    assert_eq!((decision.status, decision.kind), (DecisionStatus::Deciding, AgentKind::Kimi));
    assert_eq!(decision.question, QUESTION);
    assert_eq!(decision.context, [QUESTION], "the screen the agent stopped on");

    let handled = view_where(&mut peer, &mut window, |view| view.handled.iter().any(|entry| entry.agent == agent));
    let entry = &handled.handled[0];
    assert_eq!((entry.by, entry.outcome, entry.mode), (By::Octo, Outcome::Answered, Mode::Moderate));
    assert_eq!((entry.summary.as_str(), entry.question.as_str()), ("Told it to run the tests", QUESTION));
    assert_eq!(entry.kind, AgentKind::Kimi);
    assert!(handled.decisions.is_empty(), "an answered decision leaves the queue");
    assert_eq!(handled.handled_today, 1);

    let answer = cwd.join(ANSWER_FILE);
    let deadline = Instant::now() + TIMEOUT;
    while !answer.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(std::fs::read_to_string(&answer).unwrap(), "yes", "the answer of the engine reached the agent");

    let prompt = std::fs::read_to_string(fx.dir.join(PROMPT_FILE)).unwrap();
    assert!(prompt.contains("You are Octo") && prompt.contains("Mode: moderate."), "{prompt}");
    assert!(prompt.contains(QUESTION), "the engine read the screen of the agent: {prompt}");
    let home = fx.store.home();
    assert!(home.orchestrator_dir().join("OCTO.md").is_file(), "the global instructions exist once they are read");
    assert!(
        std::fs::read_dir(home.orchestrator_dir().join("work")).unwrap().next().is_none(),
        "the engine ran in an empty folder"
    );
    let journal = home.orchestrator_dir().join("activity.jsonl");
    while !journal.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let journal = std::fs::read_to_string(&journal).unwrap();
    let logged: Vec<ActivityEntry> = journal.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(logged, std::slice::from_ref(entry), "the activity is on disk as it was shown");

    state_of(&mut peer, &mut window, "asks", AgentState::Done);
    let after = fx.hub.orchestrator().unwrap().view();
    assert!(after.decisions.is_empty());
    assert_eq!(after.handled.len(), 1, "the pause Octo answered is not logged again when it ends");
}
