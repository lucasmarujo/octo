use std::io;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use orkai::pwa::bridge::{self, Info, Phase};
use orkai::pwa::hub::Hub;
use orkai::pwa::qr::Qr;
use serde::Serialize;
use tauri::State;

use crate::link::Link;

const CHECK_EVERY: Duration = Duration::from_secs(1);
const REVIVE_AFTER: Duration = Duration::from_secs(20);
const STOP_WAIT: Duration = Duration::from_secs(10);

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub enabled: bool,
    pub phase: Option<Phase>,
    pub tunnel: Option<String>,
    pub error: Option<String>,
    pub phones: usize,
    pub link: Option<String>,
    pub qr: Option<Vec<String>>,
}

pub fn qr_rows(qr: &Qr) -> Vec<String> {
    (0..qr.size()).map(|y| (0..qr.size()).map(|x| if qr.is_dark(x, y) { '1' } else { '0' }).collect()).collect()
}

pub fn describe(enabled: bool, info: Option<&Info>, link: Option<String>) -> Pairing {
    let qr = link.as_deref().and_then(Qr::encode).map(|qr| qr_rows(&qr));
    Pairing {
        enabled,
        phase: info.map(|info| info.phase),
        tunnel: info.map(|info| info.tunnel.label().to_string()),
        error: info.and_then(|info| info.error.clone()),
        phones: info.map_or(0, |info| info.phones),
        link,
        qr,
    }
}

pub fn read(hub: &Hub) -> Pairing {
    let home = hub.store().home();
    let info = bridge::status(home);
    let link = info
        .as_ref()
        .and_then(Info::ready_url)
        .and_then(|url| bridge::load_secret(home).ok().map(|secret| bridge::pairing_link(url, &secret)));
    describe(hub.store().pwa_enabled().unwrap_or(false), info.as_ref(), link)
}

fn start(hub: &Hub) -> io::Result<bool> {
    let launcher = hub.launcher();
    let config = launcher.config();
    bridge::start(hub.store().home(), launcher.holder_exe(), config.pwa_tunnel, config.pwa_port)
}

pub fn set_enabled(hub: &Hub, on: bool) -> io::Result<()> {
    let mut ui = hub.store().load_ui();
    ui.pwa = on;
    hub.store().save_ui(&ui)?;
    if on { start(hub).map(|_| ()) } else { bridge::stop(hub.store().home()) }
}

pub fn renew(hub: &Hub) -> io::Result<()> {
    let home = hub.store().home();
    bridge::stop(home)?;
    if !bridge::wait_until(home, STOP_WAIT, |info| info.is_none()) {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "the old bridge did not stop"));
    }
    bridge::forget_phones(home)?;
    start(hub).map(|_| ())
}

pub fn keep_alive(hub: Arc<Hub>) {
    let _ = thread::Builder::new().name("orkai-desktop-pwa".into()).spawn(move || {
        let mut started: Option<Instant> = None;
        loop {
            let enabled = hub.store().pwa_enabled().unwrap_or(false);
            let due = started.is_none_or(|at| at.elapsed() >= REVIVE_AFTER);
            if enabled && due && bridge::status(hub.store().home()).is_none() {
                started = Some(Instant::now());
                let _ = start(&hub);
            }
            thread::sleep(CHECK_EVERY);
        }
    });
}

#[tauri::command]
pub fn pairing(link: State<'_, Link>) -> Pairing {
    read(link.hub())
}

#[tauri::command(async)]
pub fn set_phone(link: State<'_, Link>, on: bool) -> Result<Pairing, String> {
    set_enabled(link.hub(), on).map_err(|err| err.to_string())?;
    Ok(read(link.hub()))
}

#[tauri::command(async)]
pub fn renew_phone(link: State<'_, Link>) -> Result<Pairing, String> {
    renew(link.hub()).map_err(|err| err.to_string())?;
    Ok(read(link.hub()))
}

#[cfg(test)]
mod tests {
    use orkai::pwa::tunnel::TunnelKind;

    use super::*;

    fn info(phase: Phase, phones: usize) -> Info {
        Info {
            pid: 7,
            port: 7470,
            tunnel: TunnelKind::Cloudflare,
            phase,
            url: Some("https://orkai.example".into()),
            error: None,
            phones,
            started_at_ms: 1,
            alive_at_ms: 2,
            exe: None,
            version: bridge::VERSION,
        }
    }

    #[test]
    fn a_ready_bridge_shows_a_scannable_code_for_its_link() {
        let link = "https://orkai.example/#k=secret".to_string();
        let pairing = describe(true, Some(&info(Phase::Ready, 1)), Some(link.clone()));
        assert_eq!(pairing.link.as_deref(), Some(link.as_str()));
        assert_eq!(pairing.phones, 1);
        assert_eq!(pairing.tunnel.as_deref(), Some(TunnelKind::Cloudflare.label()));
        let rows = pairing.qr.unwrap();
        let qr = Qr::encode(&link).unwrap();
        assert_eq!(rows.len(), qr.size());
        assert!(rows.iter().all(|row| row.len() == qr.size() && row.chars().all(|c| c == '0' || c == '1')));
        assert_eq!(rows[0].starts_with('1'), qr.is_dark(0, 0));
    }

    #[test]
    fn a_bridge_still_opening_its_tunnel_has_no_code_yet() {
        let pairing = describe(true, Some(&info(Phase::Connecting, 0)), None);
        assert_eq!(pairing.phase, Some(Phase::Connecting));
        assert_eq!(pairing.qr, None);
        let off = describe(false, None, None);
        assert_eq!(
            off,
            Pairing { enabled: false, phase: None, tunnel: None, error: None, phones: 0, link: None, qr: None }
        );
    }

    #[test]
    fn the_phase_reaches_the_window_in_lowercase() {
        let json = serde_json::to_value(describe(true, Some(&info(Phase::Failed, 0)), None)).unwrap();
        assert_eq!(json["phase"], "failed");
    }
}
