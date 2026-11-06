use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::fsutil;
use crate::launch;
use crate::model::{AgentId, ExitInfo, RuntimeInfo};
use crate::paths::{HOME_DIR, HOME_ENV, Home, LEGACY_HOME_DIR};
use crate::pwa::bridge;

const RENAME_ATTEMPTS: u32 = 20;
const RENAME_BACKOFF: Duration = Duration::from_millis(100);
const BRIDGE_STOP_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
pub enum Adoption {
    Settled,
    Moved,
    Linked,
}

pub fn adopt_on_launch() {
    if std::env::var_os(HOME_ENV).is_some_and(|dir| !dir.is_empty()) {
        return;
    }
    let Some(base) = dirs::data_local_dir() else { return };
    match adopt(&base, launch::process_is_holder) {
        Ok(Adoption::Settled) => {}
        Ok(done) => {
            bridge::diag(&Home::at(base.join(HOME_DIR)), &format!("moving the data folder to {HOME_DIR}: {done:?}"))
        }
        Err(err) => bridge::diag(&Home::at(base.join(LEGACY_HOME_DIR)), &format!("could not link {HOME_DIR}: {err}")),
    }
}

pub fn adopt(base: &Path, is_holder: impl Fn(u32) -> bool) -> io::Result<Adoption> {
    let (home, legacy) = (base.join(HOME_DIR), base.join(LEGACY_HOME_DIR));
    if is_link(&home) && !home.exists() {
        remove_link(&home)?;
    }
    if !legacy.is_dir() {
        return Ok(Adoption::Settled);
    }
    let linked = is_link(&home) && same_folder(&home, &legacy);
    if home.exists() && !linked {
        return Ok(Adoption::Settled);
    }
    if is_free(&Home::at(&legacy), &is_holder) {
        if linked {
            remove_link(&home)?;
        }
        if rename(&legacy, &home).is_ok() {
            return Ok(Adoption::Moved);
        }
    }
    if !home.exists() {
        create_link(&home, &legacy)?;
    }
    Ok(Adoption::Linked)
}

fn is_free(legacy: &Home, is_holder: &impl Fn(u32) -> bool) -> bool {
    if has_worktrees(legacy) || has_running_agent(legacy, is_holder) {
        return false;
    }
    if bridge::status(legacy).is_none() {
        return true;
    }
    bridge::stop(legacy).is_ok() && bridge::wait_until(legacy, BRIDGE_STOP_WAIT, |info| info.is_none())
}

fn has_worktrees(home: &Home) -> bool {
    fs::read_dir(home.worktrees_dir()).is_ok_and(|mut entries| entries.next().is_some())
}

fn has_running_agent(home: &Home, is_holder: &impl Fn(u32) -> bool) -> bool {
    let Ok(entries) = fs::read_dir(home.agents_dir()) else { return false };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().and_then(|name| name.parse::<AgentId>().ok()))
        .filter(|id| !matches!(fsutil::read_json_opt::<ExitInfo>(&home.exit_file(*id)), Ok(Some(_))))
        .filter_map(|id| fsutil::read_json_opt::<RuntimeInfo>(&home.runtime_file(id)).ok().flatten())
        .any(|runtime| is_holder(runtime.holder_pid))
}

fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match fs::rename(from, to) {
            Err(_) if attempt < RENAME_ATTEMPTS => {
                attempt += 1;
                thread::sleep(RENAME_BACKOFF);
            }
            result => return result,
        }
    }
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn same_folder(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

#[cfg(windows)]
fn remove_link(link: &Path) -> io::Result<()> {
    fs::remove_dir(link)
}

#[cfg(unix)]
fn remove_link(link: &Path) -> io::Result<()> {
    fs::remove_file(link)
}

#[cfg(windows)]
fn create_link(link: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let status = launch::without_console(&mut Command::new("cmd"))
        .raw_arg(format!("/d /c mklink /J \"{}\" \"{}\"", link.display(), target.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() { Ok(()) } else { Err(io::Error::other(format!("mklink ended with {status}"))) }
}

#[cfg(unix)]
fn create_link(link: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::AgentKind;
    use crate::paths::located_in;
    use crate::protocol::PROTOCOL_VERSION;
    use crate::store::Store;
    use crate::util::now_ms;

    const LIVE_PID: u32 = 4242;

    struct Fixture {
        base: PathBuf,
        home: PathBuf,
        legacy: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if is_link(&self.home) {
                let _ = remove_link(&self.home);
            }
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    fn fixture(tag: &str) -> Fixture {
        let base = std::env::temp_dir().join(format!("octo-migrate {tag}-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(&base).unwrap();
        Fixture { home: base.join(HOME_DIR), legacy: base.join(LEGACY_HOME_DIR), base }
    }

    fn with_config(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("config.json"), b"{}").unwrap();
    }

    fn running_agent(legacy: &Path, pid: u32) -> AgentId {
        let store = Store::open(Home::at(legacy)).unwrap();
        let draft = crate::model::NewAgent {
            session: "default".into(),
            name: "a".into(),
            kind: AgentKind::Terminal,
            cwd: legacy.to_path_buf(),
            task: String::new(),
        };
        let id = store.create_agent(draft).unwrap().id;
        let runtime = RuntimeInfo {
            holder_pid: pid,
            child_pid: None,
            port: 1,
            token: "t".into(),
            started_at_ms: 1,
            protocol: PROTOCOL_VERSION,
            version: "test".into(),
            exe: None,
        };
        fsutil::write_json_atomic(&store.home().runtime_file(id), &runtime).unwrap();
        id
    }

    fn live(pid: u32) -> bool {
        pid == LIVE_PID
    }

    #[test]
    fn a_new_install_has_nothing_to_move_and_starts_in_the_octo_folder() {
        let fx = fixture("new");
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Settled);
        assert_eq!(located_in(&fx.base), fx.home);
        assert!(!fx.home.exists(), "the store creates it, with its private permissions");
    }

    #[test]
    fn an_idle_folder_moves_whole_with_a_single_rename() {
        let fx = fixture("idle");
        with_config(&fx.legacy);
        let id = running_agent(&fx.legacy, LIVE_PID + 1);
        assert_eq!(located_in(&fx.base), fx.legacy, "until it moves the old folder is the one in use");
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Moved);
        assert!(!is_link(&fx.home) && fx.home.join("config.json").is_file());
        assert!(Home::at(&fx.home).agent_file(id).is_file());
        assert!(!fx.legacy.exists());
        assert_eq!(located_in(&fx.base), fx.home);
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Settled, "a second launch has nothing to do");
    }

    #[test]
    fn a_running_agent_keeps_the_old_folder_and_octo_points_at_it_until_it_is_gone() {
        let fx = fixture("running");
        with_config(&fx.legacy);
        let id = running_agent(&fx.legacy, LIVE_PID);
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Linked);
        assert!(is_link(&fx.home), "the old folder stays where the holder has it open");
        assert!(fx.legacy.join("config.json").is_file());
        assert!(fx.home.join("config.json").is_file(), "the same files, under the new name");
        assert_eq!(located_in(&fx.base), fx.home);
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Linked, "still running at the next launch");

        fsutil::write_json_atomic(
            &Home::at(&fx.home).exit_file(id),
            &ExitInfo { code: Some(0), error: None, at_ms: 1 },
        )
        .unwrap();
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Moved);
        assert!(!is_link(&fx.home) && fx.home.join("config.json").is_file());
        assert!(!fx.legacy.exists());
    }

    #[test]
    fn a_worktree_keeps_the_old_folder_since_git_and_the_agent_record_its_path() {
        let fx = fixture("worktree");
        with_config(&fx.legacy);
        let worktree = Home::at(&fx.legacy).worktree_dir(3);
        fs::create_dir_all(&worktree).unwrap();
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Linked);
        assert!(worktree.is_dir());

        fs::remove_dir_all(&worktree).unwrap();
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Moved, "an empty worktrees folder holds nothing back");
    }

    #[test]
    fn an_octo_folder_that_exists_is_never_touched_and_never_merged() {
        let fx = fixture("both");
        with_config(&fx.legacy);
        fs::create_dir_all(&fx.home).unwrap();
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Settled);
        assert!(fx.legacy.join("config.json").is_file());
        assert!(!fx.home.join("config.json").exists());
    }

    #[test]
    fn a_link_the_person_made_to_somewhere_else_stays() {
        let fx = fixture("own-link");
        with_config(&fx.legacy);
        let elsewhere = fx.base.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        create_link(&fx.home, &elsewhere).unwrap();
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Settled);
        assert!(is_link(&fx.home) && same_folder(&fx.home, &elsewhere));
        assert!(fx.legacy.is_dir());
    }

    #[test]
    fn a_link_left_without_its_folder_does_not_stop_the_app_from_opening() {
        let fx = fixture("dangling");
        with_config(&fx.legacy);
        create_link(&fx.home, &fx.legacy).unwrap();
        fs::remove_dir_all(&fx.legacy).unwrap();
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Settled);
        assert!(!is_link(&fx.home));
        Store::open(Home::at(located_in(&fx.base))).unwrap();
        assert!(fx.home.is_dir());
    }

    #[test]
    fn a_home_with_spaces_and_an_ampersand_in_its_path_still_links() {
        let fx = fixture("a&b");
        with_config(&fx.legacy);
        running_agent(&fx.legacy, LIVE_PID);
        assert_eq!(adopt(&fx.base, live).unwrap(), Adoption::Linked);
        assert!(fx.home.join("config.json").is_file());
    }
}
