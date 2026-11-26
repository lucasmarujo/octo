use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use orkai::launch::without_console;
use orkai::pwa::serve;
use orkai::update::{Asset, BASE_URL_ENV, Manifest, Source};
use orkai::util::random_bytes;
use serde::{Deserialize, Serialize};

use crate::remote::{Address, Phase, RETRY, RETRY_MAX, Remote};

pub const X86_64: &str = "x86_64";
pub const AARCH64: &str = "aarch64";
pub const ASSET_X86_64: &str = "server-x86_64-unknown-linux-musl";
pub const ASSET_AARCH64: &str = "server-aarch64-unknown-linux-musl";
pub const INSTALL_DIR: &str = ".octo/bin";
pub const DEFAULT_PORT: u16 = 22;
const RELEASES: &str = "https://github.com/lucasmarujo/octo/releases/download";
const UPLOAD_FILE: &str = ".octo-server-upload";
const UPLOAD_COMMAND: &str = "cat > .octo-server-upload";
const SHELL_COMMAND: &str = "sh -s";
const PROGRAM: &str = "ssh";
const ASKPASS_ENV: &str = "OCTO_ASKPASS";
const SECRET_ENV: &str = "OCTO_ASKPASS_SECRET";
const INSTALLING: &str = "octo-installing";
const ARCH: &str = "octo-arch";
const FAILED_OS: &str = "os";
const FAILED_ARCH: &str = "arch";
const FAILED_DOWNLOAD: &str = "download";
const FAILED_CHECKSUM: &str = "checksum";
const FAILED_HOME: &str = "home";
const POLL: Duration = Duration::from_millis(100);
const FAR_PORTS: u16 = 40_000;
const FIRST_FAR_PORT: u16 = 20_000;
const MAX_HOST: usize = 253;
const MAX_USER: usize = 64;
const KEPT_ERRORS: usize = 12;
const MAX_NOTE: usize = 200;
const SHA256_HEX: usize = 64;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub user: String,
}

fn plain(text: &str, max: usize, allowed: &[char]) -> bool {
    !text.is_empty()
        && text.len() <= max
        && !text.starts_with('-')
        && text.chars().all(|c| c.is_ascii_alphanumeric() || allowed.contains(&c))
}

impl Server {
    pub fn parse(host: &str, port: u16, user: &str) -> Result<Self, String> {
        let (host, user) = (host.trim(), user.trim());
        if !plain(host, MAX_HOST, &['.', '-', '_', ':']) {
            return Err("that is not a host name or an address".into());
        }
        if !plain(user, MAX_USER, &['.', '-', '_']) {
            return Err("that is not a user name".into());
        }
        if port == 0 {
            return Err("that is not a port".into());
        }
        Ok(Self { host: host.to_ascii_lowercase(), port, user: user.to_string() })
    }

    pub fn label(&self) -> String {
        if self.port == DEFAULT_PORT {
            format!("{}@{}", self.user, self.host)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    base: String,
    version: String,
    x86_64: Asset,
    aarch64: Asset,
}

fn base_url() -> String {
    std::env::var(BASE_URL_ENV)
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| format!("{RELEASES}/v{}", env!("CARGO_PKG_VERSION")))
}

fn sound(asset: &Asset) -> bool {
    plain(&asset.name, MAX_HOST, &['.', '-', '_'])
        && asset.sha256.len() == SHA256_HEX
        && asset.sha256.chars().all(|c| c.is_ascii_hexdigit())
}

impl Release {
    pub fn fetch() -> io::Result<Self> {
        let base = base_url();
        Self::of(&Source::at(&base).manifest()?, &base)
    }

    pub fn of(manifest: &Manifest, base: &str) -> io::Result<Self> {
        let asset = |key: &str| {
            manifest
                .assets
                .get(key)
                .filter(|asset| sound(asset))
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("this release has no {key}")))
        };
        let base = base.trim_end_matches('/');
        if base.chars().any(|c| c.is_whitespace() || c.is_control() || ['"', '\'', '$', '`', '\\'].contains(&c)) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the address of the release is not usable"));
        }
        if !plain(&manifest.version, MAX_USER, &['.', '-']) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the version of the release is not usable"));
        }
        Ok(Self {
            base: base.to_string(),
            version: manifest.version.clone(),
            x86_64: asset(ASSET_X86_64)?,
            aarch64: asset(ASSET_AARCH64)?,
        })
    }

    fn asset(&self, arch: &str) -> Option<&Asset> {
        match arch {
            X86_64 | "amd64" => Some(&self.x86_64),
            AARCH64 | "arm64" => Some(&self.aarch64),
            _ => None,
        }
    }

    pub fn script(&self, port: u16) -> String {
        let failed = serve::FAILED;
        format!(
            r#"export ORKAI_NO_UPDATE=1
os=$(uname -s)
arch=$(uname -m)
if [ "$os" != Linux ]; then echo "{failed} {FAILED_OS} $os"; exit 3; fi
case "$arch" in
  x86_64|amd64) name="{x_name}"; want="{x_sha}" ;;
  aarch64|arm64) name="{a_name}"; want="{a_sha}" ;;
  *) echo "{failed} {FAILED_ARCH} $arch"; exit 3 ;;
esac
echo "{ARCH} $arch"
dir="$HOME/{INSTALL_DIR}"
bin="$dir/$name-{version}"
sent="$HOME/{UPLOAD_FILE}"
sum() {{
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1
  fi
}}
mkdir -p "$dir" || {{ echo "{failed} {FAILED_HOME} $dir"; exit 3; }}
if [ -f "$sent" ]; then mv -f "$sent" "$bin.new"; fi
if [ ! -x "$bin" ] || [ "$(sum "$bin" 2>/dev/null)" != "$want" ]; then
  echo "{INSTALLING} $bin"
  if [ ! -f "$bin.new" ]; then
    if command -v curl >/dev/null 2>&1; then curl -fsSL --connect-timeout 15 -o "$bin.new" "{base}/$name"
    elif command -v wget >/dev/null 2>&1; then wget -q -T 15 -O "$bin.new" "{base}/$name"
    else false
    fi || {{ rm -f "$bin.new"; echo "{failed} {FAILED_DOWNLOAD} $arch"; exit 4; }}
  fi
  if [ "$(sum "$bin.new" 2>/dev/null)" != "$want" ]; then rm -f "$bin.new"; echo "{failed} {FAILED_CHECKSUM} $name"; exit 5; fi
  chmod 700 "$bin.new" && mv -f "$bin.new" "$bin" || {{ echo "{failed} {FAILED_HOME} $dir"; exit 3; }}
fi
rm -f "$bin.new"
for old in "$dir"/octo-server-*; do
  [ "$old" = "$bin" ] || rm -f "$old"
done
exec "$bin" __serve --port {port}
"#,
            x_name = self.x86_64.name,
            x_sha = self.x86_64.sha256.to_ascii_lowercase(),
            a_name = self.aarch64.name,
            a_sha = self.aarch64.sha256.to_ascii_lowercase(),
            version = self.version,
            base = self.base,
        )
    }
}

pub fn answer<'a>(prompt: &str, secret: &'a str) -> Option<&'a str> {
    let prompt = prompt.to_ascii_lowercase();
    (prompt.contains("password") && !prompt.contains("passphrase")).then_some(secret)
}

pub fn askpass() -> Option<ExitCode> {
    std::env::var_os(ASKPASS_ENV)?;
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let secret = std::env::var(SECRET_ENV).unwrap_or_default();
    Some(match answer(&prompt, &secret) {
        Some(secret) => {
            println!("{secret}");
            ExitCode::SUCCESS
        }
        None => ExitCode::FAILURE,
    })
}

pub fn arguments(server: &Server, with_password: bool, forward: Option<(u16, u16)>, command: &str) -> Vec<String> {
    let methods = if with_password { "publickey,keyboard-interactive,password" } else { "publickey" };
    let mut args: Vec<String> =
        vec!["-T".into(), "-p".into(), server.port.to_string(), "-l".into(), server.user.clone()];
    for option in [
        format!("BatchMode={}", if with_password { "no" } else { "yes" }),
        format!("PreferredAuthentications={methods}"),
        "NumberOfPasswordPrompts=1".into(),
        "StrictHostKeyChecking=accept-new".into(),
        "ConnectTimeout=10".into(),
        "ServerAliveInterval=15".into(),
        "ServerAliveCountMax=3".into(),
        "ExitOnForwardFailure=yes".into(),
        "LogLevel=ERROR".into(),
    ] {
        args.extend(["-o".into(), option]);
    }
    if let Some((local, far)) = forward {
        args.extend(["-L".into(), format!("127.0.0.1:{local}:127.0.0.1:{far}")]);
    }
    args.extend(["--".into(), server.host.clone(), command.into()]);
    args
}

fn start(server: &Server, password: Option<&str>, forward: Option<(u16, u16)>, command: &str) -> io::Result<Child> {
    let mut ssh = Command::new(PROGRAM);
    ssh.args(arguments(server, password.is_some(), forward, command))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(password) = password {
        ssh.env("SSH_ASKPASS", std::env::current_exe()?)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env(ASKPASS_ENV, "1")
            .env(SECRET_ENV, password);
        if std::env::var_os("DISPLAY").is_none() {
            ssh.env("DISPLAY", ":0");
        }
    }
    without_console(&mut ssh).spawn()
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub up: bool,
    pub arch: Option<String>,
    pub failure: Option<(String, String)>,
    pub errors: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Stop(Phase, String),
    Send(String),
    Retry(String),
}

fn clip(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MAX_NOTE).collect::<String>().trim().to_string()
}

pub fn judge(seen: &Seen, with_password: bool) -> Verdict {
    let said = |needle: &str| seen.errors.iter().any(|line| line.contains(needle));
    if let Some((kind, detail)) = &seen.failure {
        return match kind.as_str() {
            FAILED_OS => Verdict::Stop(
                Phase::Unsupported,
                format!("Octo only runs on Linux servers for now; this one is {detail}"),
            ),
            FAILED_ARCH => Verdict::Stop(
                Phase::Unsupported,
                format!("Octo has no server for {detail} yet; it runs on x86_64 and arm64"),
            ),
            FAILED_DOWNLOAD => Verdict::Send(detail.clone()),
            FAILED_CHECKSUM => Verdict::Retry("the server Octo downloaded there does not match this release".into()),
            FAILED_HOME => Verdict::Stop(Phase::Unsupported, format!("Octo could not write to {detail} on the server")),
            serve::PORT_TAKEN => Verdict::Retry("the port was taken on the server".into()),
            _ => Verdict::Retry(clip(detail)),
        };
    }
    if said("HOST IDENTIFICATION HAS CHANGED") || said("Host key verification failed") {
        return Verdict::Stop(Phase::Changed, "the key of this server changed since the last time".into());
    }
    if said("Permission denied") || said("Too many authentication failures") {
        let note = if with_password { "that password was not accepted" } else { "this server asks for a password" };
        return Verdict::Stop(Phase::Password, note.into());
    }
    if seen.up {
        return Verdict::Retry("the connection to the server closed".into());
    }
    Verdict::Retry(
        seen.errors
            .last()
            .map(|line| clip(line))
            .filter(|line| !line.is_empty())
            .unwrap_or_else(|| "the server did not answer".into()),
    )
}

pub fn hear(seen: &mut Seen, line: &str) {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(serve::FAILED) {
        let mut parts = rest.trim().splitn(2, ' ');
        let kind = parts.next().unwrap_or_default().to_string();
        seen.failure = Some((kind, clip(parts.next().unwrap_or_default())));
    } else if let Some(arch) = line.strip_prefix(ARCH) {
        seen.arch = Some(clip(arch));
    }
}

enum Event {
    Out(String),
    Err(String),
}

fn pump(stream: impl Read + Send + 'static, events: Sender<Event>, wrap: fn(String) -> Event) {
    let _ = thread::Builder::new().name("orkai-desktop-ssh-read".into()).spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
            if events.send(wrap(String::from_utf8_lossy(&line).into_owned())).is_err() {
                return;
            }
            line.clear();
        }
    });
}

pub enum Control {
    Unlock(String),
    Retry,
}

enum Ended {
    Gone,
    Asked(Control),
    Judged(Verdict),
}

fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?.local_addr()?.port())
}

fn far_port() -> io::Result<u16> {
    Ok(FIRST_FAR_PORT + u16::from_be_bytes(random_bytes::<2>()?) % FAR_PORTS)
}

fn finish(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn session(
    server: &Server,
    remote: &Remote,
    password: Option<&str>,
    release: &Release,
    control: &Receiver<Control>,
) -> Ended {
    let opened = free_port().and_then(|local| Ok((local, far_port()?))).and_then(|(local, far)| {
        let mut child = start(server, password, Some((local, far)), SHELL_COMMAND)?;
        let fed = child
            .stdin
            .as_mut()
            .map(|stdin| stdin.write_all(release.script(far).as_bytes()).and_then(|()| stdin.flush()));
        if let Some(Err(err)) = fed {
            finish(&mut child);
            return Err(err);
        }
        Ok((child, local))
    });
    let (mut child, local) = match opened {
        Ok(opened) => opened,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ended::Judged(Verdict::Retry("the ssh program is not installed on this computer".into()));
        }
        Err(err) => return Ended::Judged(Verdict::Retry(clip(&err.to_string()))),
    };
    let (events, heard) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        pump(stdout, events.clone(), Event::Out);
    }
    if let Some(stderr) = child.stderr.take() {
        pump(stderr, events, Event::Err);
    }
    let mut seen = Seen::default();
    let note = |seen: &mut Seen, event: Event| match event {
        Event::Out(line) => {
            if let Some((_, secret)) = serve::parse_ready(&line)
                && let Some(address) = Address::parse(&format!("http://127.0.0.1:{local}"))
            {
                seen.up = true;
                remote.aim(address, secret);
            } else if let Some(path) = line.trim().strip_prefix(INSTALLING) {
                remote.park(Phase::Installing, Some(clip(path)));
            } else {
                hear(seen, &line);
            }
        }
        Event::Err(line) => {
            seen.errors.push(clip(&line));
            if seen.errors.len() > KEPT_ERRORS {
                seen.errors.remove(0);
            }
        }
    };
    loop {
        match control.try_recv() {
            Ok(asked) => {
                finish(&mut child);
                return Ended::Asked(asked);
            }
            Err(TryRecvError::Disconnected) => {
                finish(&mut child);
                return Ended::Gone;
            }
            Err(TryRecvError::Empty) => {}
        }
        match heard.recv_timeout(POLL) {
            Ok(event) => note(&mut seen, event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => thread::sleep(POLL),
        }
        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
            while let Ok(event) = heard.recv_timeout(POLL) {
                note(&mut seen, event);
            }
            return Ended::Judged(judge(&seen, password.is_some()));
        }
    }
}

fn send(server: &Server, password: Option<&str>, release: &Release, arch: &str) -> io::Result<()> {
    let asset = release
        .asset(arch)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("Octo has no server for {arch}")))?;
    let bytes = Source::at(&release.base).download(asset)?;
    let mut child = start(server, password, None, UPLOAD_COMMAND)?;
    let written = child.stdin.take().map(|mut stdin| stdin.write_all(&bytes).and_then(|()| stdin.flush()));
    let status = child.wait()?;
    match written {
        Some(Ok(())) if status.success() => Ok(()),
        Some(Err(err)) => Err(err),
        _ => Err(io::Error::other("the server did not take the file")),
    }
}

fn wait(control: &Receiver<Control>, delay: Option<Duration>) -> Result<Option<Control>, ()> {
    match delay {
        None => control.recv().map(Some).map_err(|_| ()),
        Some(delay) => match control.recv_timeout(delay) {
            Ok(asked) => Ok(Some(asked)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        },
    }
}

fn keep_open(server: &Server, remote: &Remote, mut password: Option<String>, control: &Receiver<Control>) {
    let mut attempt = 0;
    let mut release: Option<Release> = None;
    let mut sent = false;
    loop {
        remote.park(Phase::Connecting, None);
        let started = Instant::now();
        let ended = match release.clone().map_or_else(Release::fetch, Ok) {
            Ok(found) => {
                let ended = session(server, remote, password.as_deref(), &found, control);
                release = Some(found);
                ended
            }
            Err(err) => {
                Ended::Judged(Verdict::Retry(format!("could not read what to install: {}", clip(&err.to_string()))))
            }
        };
        let verdict = match ended {
            Ended::Gone => return,
            Ended::Asked(Control::Unlock(given)) => {
                password = Some(given);
                attempt = 0;
                continue;
            }
            Ended::Asked(Control::Retry) => {
                attempt = 0;
                continue;
            }
            Ended::Judged(verdict) => verdict,
        };
        let pause = match verdict {
            Verdict::Send(arch) if !sent => {
                sent = true;
                remote.park(Phase::Installing, Some("sending the server over this connection".into()));
                match release.as_ref().map(|release| send(server, password.as_deref(), release, &arch)) {
                    Some(Ok(())) => continue,
                    _ => {
                        let note = "neither the server nor this computer could download the Octo server";
                        remote.park(Phase::Offline, Some(note.into()));
                        Some(RETRY_MAX)
                    }
                }
            }
            Verdict::Send(_) => {
                remote.park(Phase::Offline, Some("the server could not download the Octo server".into()));
                Some(RETRY_MAX)
            }
            Verdict::Stop(phase, note) => {
                if phase == Phase::Password {
                    password = None;
                }
                remote.park(phase, Some(note));
                None
            }
            Verdict::Retry(note) => {
                if started.elapsed() > RETRY_MAX {
                    attempt = 0;
                }
                remote.park(Phase::Offline, Some(note));
                let delay = RETRY.get(attempt).copied().unwrap_or(RETRY_MAX);
                attempt += 1;
                Some(delay)
            }
        };
        match wait(control, pause) {
            Err(()) => return,
            Ok(Some(Control::Unlock(given))) => {
                password = Some(given);
                attempt = 0;
                sent = false;
            }
            Ok(Some(Control::Retry)) => {
                attempt = 0;
                sent = false;
            }
            Ok(None) => {}
        }
    }
}

pub struct Tunnel {
    control: Sender<Control>,
}

impl Tunnel {
    pub fn open(server: Server, remote: Remote, password: Option<String>) -> io::Result<Self> {
        let (control, asked) = mpsc::channel();
        thread::Builder::new()
            .name("orkai-desktop-ssh".into())
            .spawn(move || keep_open(&server, &remote, password, &asked))?;
        Ok(Self { control })
    }

    pub fn unlock(&self, password: String) {
        let _ = self.control.send(Control::Unlock(password));
    }

    pub fn retry(&self) {
        let _ = self.control.send(Control::Retry);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn asset(name: &str, fill: char) -> Asset {
        Asset { name: name.into(), sha256: fill.to_string().repeat(SHA256_HEX), size: 9 }
    }

    fn manifest() -> Manifest {
        Manifest {
            commit: "abc".into(),
            version: "0.2.46".into(),
            built_at: String::new(),
            assets: BTreeMap::from([
                (ASSET_X86_64.to_string(), asset("octo-server-linux-x86_64", 'a')),
                (ASSET_AARCH64.to_string(), asset("octo-server-linux-aarch64", 'B')),
            ]),
        }
    }

    fn server() -> Server {
        Server::parse("Build.Example", 2222, "deploy").unwrap()
    }

    #[test]
    fn a_server_is_a_plain_host_a_port_and_a_user_and_never_an_option_of_ssh() {
        assert_eq!(server(), Server { host: "build.example".into(), port: 2222, user: "deploy".into() });
        assert_eq!(server().label(), "deploy@build.example:2222");
        assert_eq!(Server::parse(" 10.0.0.7 ", DEFAULT_PORT, " root ").unwrap().label(), "root@10.0.0.7");
        assert!(Server::parse("fe80::1", 22, "pi").is_ok());
        for host in ["", "-oProxyCommand=calc", "a b", "host;rm", "$(id)", "a/b", "user@host"] {
            assert!(Server::parse(host, 22, "root").is_err(), "{host}");
        }
        for user in ["", "-l", "a b", "root;id", "a@b", "$x"] {
            assert!(Server::parse("host", 22, user).is_err(), "{user}");
        }
        assert!(Server::parse("host", 0, "root").is_err());
    }

    #[test]
    fn ssh_is_asked_for_a_tunnel_without_ever_seeing_the_password_in_its_arguments() {
        let args = arguments(&server(), true, Some((50001, 23456)), SHELL_COMMAND);
        let line = args.join(" ");
        assert!(line.starts_with("-T -p 2222 -l deploy "), "{line}");
        assert!(line.ends_with("-L 127.0.0.1:50001:127.0.0.1:23456 -- build.example sh -s"), "{line}");
        assert!(line.contains("BatchMode=no") && line.contains("StrictHostKeyChecking=accept-new"));
        assert!(line.contains("ExitOnForwardFailure=yes") && line.contains("NumberOfPasswordPrompts=1"));
        let keys = arguments(&server(), false, None, UPLOAD_COMMAND).join(" ");
        assert!(keys.contains("BatchMode=yes"), "without a password ssh never stops to ask: {keys}");
        assert!(keys.contains("PreferredAuthentications=publickey ") && !keys.contains("-L"));
        assert!(keys.ends_with("-- build.example cat > .octo-server-upload"));
    }

    #[test]
    fn the_password_only_answers_a_question_about_the_password() {
        assert_eq!(answer("deploy@build.example's password: ", "s3cret"), Some("s3cret"));
        assert_eq!(answer("(deploy@build) Password:", "s3cret"), Some("s3cret"));
        assert_eq!(answer("Enter passphrase for key '/home/me/.ssh/id_ed25519': ", "s3cret"), None);
        assert_eq!(answer("Are you sure you want to continue connecting (yes/no/[fingerprint])?", "s3cret"), None);
    }

    #[test]
    fn the_server_installs_only_under_the_home_checks_the_hash_and_then_serves() {
        let release = Release::of(&manifest(), "https://example.test/releases/v0.2.46/").unwrap();
        let script = release.script(23456);
        assert!(script.contains(r#"dir="$HOME/.octo/bin""#));
        assert!(script.contains(r#"bin="$dir/$name-0.2.46""#));
        assert!(
            script.contains(&format!(r#"x86_64|amd64) name="octo-server-linux-x86_64"; want="{}""#, "a".repeat(64)))
        );
        assert!(
            script.contains(&format!(r#"aarch64|arm64) name="octo-server-linux-aarch64"; want="{}""#, "b".repeat(64)))
        );
        assert!(script.contains(r#""https://example.test/releases/v0.2.46/$name""#));
        assert!(script.contains(r#"if [ "$os" != Linux ]; then echo "octo-error os $os"; exit 3; fi"#));
        assert!(script.contains(r#"*) echo "octo-error arch $arch"; exit 3 ;;"#));
        assert!(script.trim_end().ends_with(r#"exec "$bin" __serve --port 23456"#));
        let check = script.find(r#"!= "$want" ]; then rm -f "$bin.new""#).unwrap();
        let place = script.find(r#"mv -f "$bin.new" "$bin""#).unwrap();
        assert!(check < place, "nothing is put in place before its hash is checked");
        assert!(!script.contains("sudo"));
        for line in
            script.lines().filter(|line| line.contains("rm ") || line.contains("mv ") || line.contains("mkdir "))
        {
            assert!(
                line.contains("$bin") || line.contains("$dir") || line.contains("$sent") || line.contains("$old"),
                "{line}"
            );
        }
    }

    #[test]
    fn a_release_that_could_inject_into_the_script_is_refused() {
        assert!(Release::of(&manifest(), "https://example.test/a\"; rm -rf ~; \"").is_err());
        assert!(Release::of(&manifest(), "https://example.test/$(id)").is_err());
        let mut bad = manifest();
        bad.assets.get_mut(ASSET_X86_64).unwrap().name = "x\"; id; \"".into();
        assert!(Release::of(&bad, "https://example.test").is_err());
        let mut bad = manifest();
        bad.assets.get_mut(ASSET_AARCH64).unwrap().sha256 = "zz".into();
        assert!(Release::of(&bad, "https://example.test").is_err());
        let mut bad = manifest();
        bad.version = "1; id".into();
        assert!(Release::of(&bad, "https://example.test").is_err());
        let mut none = manifest();
        none.assets.remove(ASSET_AARCH64);
        assert!(Release::of(&none, "https://example.test").unwrap_err().to_string().contains(ASSET_AARCH64));
    }

    fn seen(lines: &[&str], errors: &[&str]) -> Seen {
        let mut seen = Seen::default();
        for line in lines {
            hear(&mut seen, line);
        }
        seen.errors = errors.iter().map(|line| line.to_string()).collect();
        seen
    }

    #[test]
    fn what_went_wrong_decides_whether_octo_asks_stops_sends_or_tries_again() {
        let denied = &["deploy@build: Permission denied (publickey,password)."];
        assert_eq!(
            judge(&seen(&[], denied), false),
            Verdict::Stop(Phase::Password, "this server asks for a password".into())
        );
        assert_eq!(
            judge(&seen(&[], denied), true),
            Verdict::Stop(Phase::Password, "that password was not accepted".into())
        );
        let changed = judge(&seen(&[], &["@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @"]), true);
        assert!(matches!(changed, Verdict::Stop(Phase::Changed, _)));
        let mac = judge(&seen(&["octo-error os Darwin"], &[]), false);
        assert_eq!(
            mac,
            Verdict::Stop(Phase::Unsupported, "Octo only runs on Linux servers for now; this one is Darwin".into())
        );
        let Verdict::Stop(Phase::Unsupported, arch) =
            judge(&seen(&["octo-arch riscv64", "octo-error arch riscv64"], &[]), false)
        else {
            panic!("an unknown processor stops")
        };
        assert!(arch.contains("riscv64") && arch.contains("x86_64 and arm64"), "{arch}");
        assert_eq!(
            judge(&seen(&["octo-arch aarch64", "octo-error download aarch64"], &[]), false),
            Verdict::Send("aarch64".into())
        );
        assert!(matches!(
            judge(&seen(&["octo-error checksum octo-server-linux-x86_64"], &[]), false),
            Verdict::Retry(_)
        ));
        assert!(matches!(judge(&seen(&["octo-error port"], &[]), false), Verdict::Retry(_)));
        let unreachable = judge(&seen(&[], &["ssh: connect to host build port 22: Connection timed out\r\n"]), false);
        assert_eq!(unreachable, Verdict::Retry("ssh: connect to host build port 22: Connection timed out".into()));
        assert_eq!(judge(&Seen::default(), false), Verdict::Retry("the server did not answer".into()));
        let up = Seen { up: true, ..Seen::default() };
        assert_eq!(judge(&up, false), Verdict::Retry("the connection to the server closed".into()));
    }
}
