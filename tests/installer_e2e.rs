#![cfg(windows)]

use std::ffi::OsStr;
use std::io::Read;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::ptr::null_mut;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use orkai::util::now_ms;
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/installer/orkai.iss");
const SETUP_NAME: &str = "orkai-setup.exe";
const ISCC_ENV: &str = "ORKAI_ISCC";
const DEFAULT_ISCC: &str = r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe";
const UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{F90DC11F-64BF-418E-BF55-59C746EC206F}_is1";
const ENVIRONMENT_KEY: &str = "Environment";
const ADD_TO_PATH: &str = "/MERGETASKS=addtopath";
const KEEP_OFF_PATH: &str = "/MERGETASKS=!addtopath";
const WINGET_DEFAULTS: &str = "/SP-";
const OLD_SETUP_PREFIX: &str = "orkai.exe.old-setup-";
const LOG_CLOSED: &str = "Log closed.";
const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
const RRF_RT_REG_SZ: u32 = 0x2;
const RRF_RT_REG_EXPAND_SZ: u32 = 0x4;
const RRF_NOEXPAND: u32 = 0x1000_0000;
const FIXED_SIGNATURE: u32 = 0xFEEF_04BD;
const TIMEOUT: Duration = Duration::from_secs(120);
static INSTALLS: Mutex<()> = Mutex::new(());
static SETUP: OnceLock<PathBuf> = OnceLock::new();

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegGetValueW(
        key: isize,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32;
}

#[link(name = "version")]
unsafe extern "system" {
    fn GetFileVersionInfoSizeW(file: *const u16, handle: *mut u32) -> u32;
    fn GetFileVersionInfoW(file: *const u16, handle: u32, size: u32, data: *mut u8) -> i32;
    fn VerQueryValueW(block: *const u8, sub_block: *const u16, buffer: *mut *mut u8, size: *mut u32) -> i32;
}

fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

fn registry_string(subkey: &str, value: &str) -> Option<String> {
    let (subkey, value) = (wide(subkey), wide(value));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, subkey.as_ptr(), value.as_ptr(), flags, null_mut(), null_mut(), &mut size)
    };
    if status != 0 {
        return None;
    }
    let mut data = vec![0u16; usize::try_from(size).unwrap().div_ceil(2)];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            flags,
            null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    assert_eq!(status, 0);
    data.truncate(usize::try_from(size).unwrap() / 2);
    Some(String::from_utf16_lossy(&data).trim_end_matches('\0').to_string())
}

struct VersionInfo {
    block: Vec<u32>,
}

impl VersionInfo {
    fn of(file: &Path) -> Self {
        let path = wide(file);
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), null_mut()) };
        assert!(size > 0, "{} has no version resource", file.display());
        let mut block = vec![0u32; usize::try_from(size).unwrap().div_ceil(4)];
        assert_ne!(unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, block.as_mut_ptr().cast()) }, 0);
        Self { block }
    }

    fn query(&self, sub_block: &str) -> Option<(*const u8, usize)> {
        let sub_block = wide(sub_block);
        let mut value = null_mut();
        let mut size = 0u32;
        let found = unsafe { VerQueryValueW(self.block.as_ptr().cast(), sub_block.as_ptr(), &mut value, &mut size) };
        (found != 0 && size > 0).then(|| (value.cast_const(), usize::try_from(size).unwrap()))
    }

    fn string(&self, name: &str) -> Option<String> {
        let (translation, _) = self.query(r"\VarFileInfo\Translation")?;
        let [language, codepage] = unsafe { std::ptr::read_unaligned(translation.cast::<[u16; 2]>()) };
        let (text, chars) = self.query(&format!(r"\StringFileInfo\{language:04X}{codepage:04X}\{name}"))?;
        let text = unsafe { std::slice::from_raw_parts(text.cast::<u16>(), chars) };
        Some(String::from_utf16_lossy(text).trim_end_matches(['\0', ' ']).to_string())
    }

    fn fixed_product_version(&self) -> [u16; 4] {
        let (fixed, size) = self.query(r"\").expect("no VS_FIXEDFILEINFO");
        assert!(size >= 24);
        let fixed = unsafe { std::ptr::read_unaligned(fixed.cast::<[u32; 6]>()) };
        assert_eq!(fixed[0], FIXED_SIGNATURE);
        let [high, low] = [fixed[4], fixed[5]];
        [high >> 16, high & 0xFFFF, low >> 16, low & 0xFFFF].map(|part| u16::try_from(part).unwrap())
    }
}

fn package_version() -> [u16; 4] {
    [env!("CARGO_PKG_VERSION_MAJOR"), env!("CARGO_PKG_VERSION_MINOR"), env!("CARGO_PKG_VERSION_PATCH"), "0"]
        .map(|part| part.parse().unwrap())
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < TIMEOUT, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn run(program: &Path, args: &[&str]) -> ExitStatus {
    let mut child = Command::new(program).args(args).spawn().unwrap();
    let mut status = None;
    wait_until(&program.display().to_string(), || {
        status = child.try_wait().unwrap();
        status.is_some()
    });
    status.unwrap()
}

fn setup() -> &'static Path {
    SETUP.get_or_init(|| {
        let iscc = std::env::var_os(ISCC_ENV).map_or_else(|| PathBuf::from(DEFAULT_ISCC), PathBuf::from);
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("installer");
        let output = Command::new(&iscc)
            .arg("/Q")
            .arg(format!("/DBinary={}", env!("CARGO_BIN_EXE_orkai")))
            .arg(format!("/O{}", out.display()))
            .arg(SCRIPT)
            .output()
            .unwrap_or_else(|err| panic!("{}: {err}; install Inno Setup 6 or set {ISCC_ENV}", iscc.display()));
        let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success(), "ISCC failed:\n{log}");
        out.join(SETUP_NAME)
    })
}

fn user_path() -> Vec<String> {
    let path = registry_string(ENVIRONMENT_KEY, "Path").unwrap_or_default();
    path.split(';').filter(|entry| !entry.is_empty()).map(str::to_string).collect()
}

fn same_dir(entry: &str, dir: &Path) -> bool {
    matches!((std::fs::canonicalize(entry), std::fs::canonicalize(dir)), (Ok(a), Ok(b)) if a == b)
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

struct Install {
    dir: PathBuf,
    _serial: MutexGuard<'static, ()>,
}

impl Install {
    fn new(tag: &str) -> Self {
        let serial = INSTALLS.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = std::env::temp_dir().join(format!("orkai-installer-{tag}-{}-{}", std::process::id(), now_ms()));
        Self { dir, _serial: serial }
    }

    fn exe(&self) -> PathBuf {
        self.dir.join("orkai.exe")
    }

    fn uninstaller(&self) -> PathBuf {
        self.dir.join("unins000.exe")
    }

    fn uninstall_log(&self) -> PathBuf {
        self.dir.with_extension("uninstall.log")
    }

    fn install(&self, switch: &str) {
        let log = self.dir.with_extension("log");
        let (dir, log_arg) = (format!("/DIR={}", self.dir.display()), format!("/LOG={}", log.display()));
        let status = run(setup(), &["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", &dir, switch, &log_arg]);
        let log = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(status.success(), "setup exited with {status}:\n{log}");
    }

    fn uninstall(&self) {
        let log = self.uninstall_log();
        let log_arg = format!("/LOG={}", log.display());
        let status = run(&self.uninstaller(), &["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", &log_arg]);
        assert!(status.success(), "uninstaller exited with {status}");
        wait_until("the uninstaller", || {
            std::fs::read_to_string(&log).is_ok_and(|log| log.contains(LOG_CLOSED))
                && registry_string(UNINSTALL_KEY, "DisplayName").is_none()
                && !self.uninstaller().exists()
        });
    }
}

impl Drop for Install {
    fn drop(&mut self) {
        if self.uninstaller().exists() {
            let _ = Command::new(self.uninstaller()).args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]).status();
            std::thread::sleep(Duration::from_secs(2));
        }
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_file(self.dir.with_extension("log"));
        let _ = std::fs::remove_file(self.uninstall_log());
    }
}

struct OpenOrkai {
    child: Box<dyn Child + Send + Sync>,
    home: PathBuf,
}

impl OpenOrkai {
    fn start(exe: &Path) -> Self {
        let home = std::env::temp_dir().join(format!("orkai-installer-home-{}-{}", std::process::id(), now_ms()));
        let pair =
            native_pty_system().openpty(PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut command = CommandBuilder::new(exe);
        command.env("ORKAI_HOME", &home);
        command.env("ORKAI_NO_UPDATE", "1");
        command.env("ORKAI_NO_TERMINAL_PROFILE", "1");
        let child = pair.slave.spawn_command(command).unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _master = pair.master;
            let mut buf = [0u8; 4096];
            while let Ok(read) = reader.read(&mut buf) {
                if read == 0 || tx.send(read).is_err() {
                    break;
                }
            }
        });
        let mut drawn = 0;
        wait_until("orkai to draw its screen", || {
            drawn += rx.try_iter().sum::<usize>();
            drawn > 0
        });
        Self { child, home }
    }

    fn running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }
}

impl Drop for OpenOrkai {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

#[test]
fn the_windows_binary_carries_its_name_and_version() {
    let info = VersionInfo::of(Path::new(env!("CARGO_BIN_EXE_orkai")));
    assert_eq!(info.string("ProductName").as_deref(), Some(env!("CARGO_PKG_NAME")));
    assert_eq!(info.string("ProductVersion").as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(info.string("FileVersion").as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(info.string("FileDescription").as_deref(), Some(env!("CARGO_PKG_DESCRIPTION")));
    assert_eq!(info.string("OriginalFilename").as_deref(), Some("orkai.exe"));
    assert_eq!(info.fixed_product_version(), package_version());
}

#[test]
#[ignore = "needs Inno Setup's ISCC; CI runs it with --ignored"]
fn the_installer_carries_the_same_name_and_version_as_the_binary() {
    let info = VersionInfo::of(setup());
    assert_eq!(info.string("ProductName").as_deref(), Some(env!("CARGO_PKG_NAME")));
    assert_eq!(info.string("ProductVersion").as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(info.fixed_product_version(), package_version());
}

#[test]
#[ignore = "installs orkai for the current user (registry and PATH); CI runs it with --ignored"]
fn installs_for_the_user_with_orkai_on_the_path_and_uninstalls_without_a_trace() {
    let before = user_path();
    let install = Install::new("path");
    install.install(ADD_TO_PATH);

    let version = Command::new(install.exe()).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")), "{version:?}");
    assert_eq!(registry_string(UNINSTALL_KEY, "DisplayName").as_deref(), Some("orkai"));
    assert_eq!(registry_string(UNINSTALL_KEY, "DisplayVersion").as_deref(), Some(env!("CARGO_PKG_VERSION")));
    let after = user_path();
    assert_eq!(after.len(), before.len() + 1, "{after:?}");
    assert_eq!(after[..before.len()], before[..]);
    assert!(same_dir(&after[before.len()], &install.dir), "{after:?}");

    install.install(ADD_TO_PATH);
    assert_eq!(user_path(), after, "installing again must not add orkai to the PATH twice");

    install.uninstall();
    assert_eq!(user_path(), before);
    assert!(!install.dir.exists(), "{:?}", file_names(&install.dir));
}

#[test]
#[ignore = "installs orkai for the current user (registry and PATH); CI runs it with --ignored"]
fn installs_like_winget_on_the_path_under_the_entry_its_manifest_matches() {
    let before = user_path();
    let install = Install::new("winget");
    install.install(WINGET_DEFAULTS);

    assert_eq!(registry_string(UNINSTALL_KEY, "DisplayName").as_deref(), Some("orkai"));
    assert_eq!(registry_string(UNINSTALL_KEY, "Publisher").as_deref(), Some("Lucas Marujo"));
    assert_eq!(registry_string(UNINSTALL_KEY, "DisplayVersion").as_deref(), Some(env!("CARGO_PKG_VERSION")));
    let after = user_path();
    assert_eq!(after.len(), before.len() + 1, "orkai goes on the PATH by default: {after:?}");
    assert!(same_dir(&after[before.len()], &install.dir), "{after:?}");

    install.uninstall();
    assert_eq!(user_path(), before);
}

#[test]
#[ignore = "installs orkai for the current user (registry and PATH); CI runs it with --ignored"]
fn leaves_the_path_alone_when_the_option_is_unchecked() {
    let before = user_path();
    let install = Install::new("nopath");
    install.install(KEEP_OFF_PATH);
    assert!(install.exe().exists());
    assert_eq!(user_path(), before);

    install.install(ADD_TO_PATH);
    assert_eq!(user_path().len(), before.len() + 1);
    install.install(KEEP_OFF_PATH);
    assert_eq!(user_path(), before, "unchecking the option on a new install takes orkai off the PATH");

    install.uninstall();
    assert_eq!(user_path(), before);
}

#[test]
#[ignore = "installs orkai for the current user (registry and PATH); CI runs it with --ignored"]
fn upgrades_while_orkai_is_open_without_closing_it() {
    let install = Install::new("open");
    install.install(KEEP_OFF_PATH);
    let mut open = OpenOrkai::start(&install.exe());

    install.install(KEEP_OFF_PATH);
    assert!(open.running(), "the installer must not close the open orkai");
    assert_eq!(std::fs::read(install.exe()).unwrap(), std::fs::read(env!("CARGO_BIN_EXE_orkai")).unwrap());
    let old: Vec<String> = file_names(&install.dir).into_iter().filter(|n| n.starts_with(OLD_SETUP_PREFIX)).collect();
    assert_eq!(old.len(), 1, "the open orkai.exe is moved aside: {:?}", file_names(&install.dir));

    drop(open);
    install.uninstall();
    assert!(!install.dir.exists(), "{:?}", file_names(&install.dir));
}
