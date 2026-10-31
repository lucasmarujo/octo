use std::path::PathBuf;

use orkai::config::Config;
use orkai::model::{AgentId, AgentKind, AgentState};
use orkai::notify::{self, Alert, Notifier};
use orkai::sound::{Chime, Player};
use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_notification::NotificationExt;

use crate::link::Link;

const NOTIFICATION_TITLE: &str = "orkai";

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AlertView {
    pub id: AgentId,
    pub name: String,
    pub kind: AgentKind,
    pub state: AgentState,
    pub session: String,
    pub cwd: String,
    pub activity: String,
}

impl From<AlertView> for Alert {
    fn from(view: AlertView) -> Self {
        Alert {
            id: view.id,
            name: view.name,
            kind: view.kind,
            state: view.state,
            session: view.session,
            cwd: PathBuf::from(view.cwd),
            activity: view.activity,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Delivery {
    pub chime: Option<Chime>,
    pub notification: Option<String>,
}

pub fn plan(config: &Config, alerts: &[Alert]) -> Delivery {
    if !config.notify {
        return Delivery { chime: None, notification: None };
    }
    let chime = if config.notify_sound { Chime::for_alerts(alerts) } else { None };
    Delivery { chime, notification: notify::summary(alerts) }
}

#[tauri::command(async)]
pub fn alert(app: AppHandle, link: State<'_, Link>, alerts: Vec<AlertView>) {
    let hub = link.hub();
    let launcher = hub.launcher();
    let alerts: Vec<Alert> = alerts.into_iter().map(Alert::from).collect();
    let delivery = plan(launcher.config(), &alerts);
    if let Some(chime) = delivery.chime
        && let Some(player) = Player::new(hub.store().home())
    {
        player.play(chime);
    }
    if let Some(body) = delivery.notification {
        let _ = app.notification().builder().title(NOTIFICATION_TITLE).body(body).show();
    }
    let notifier = Notifier::start(launcher.config());
    for alert in alerts {
        notifier.send(alert);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(name: &str, state: AgentState) -> Alert {
        AlertView {
            id: 1,
            name: name.into(),
            kind: AgentKind::Claude,
            state,
            session: "default".into(),
            cwd: "/work".into(),
            activity: String::new(),
        }
        .into()
    }

    #[test]
    fn the_window_sends_alerts_the_core_understands() {
        let view: AlertView = serde_json::from_str(
            r#"{"id":4,"name":"fix-login","kind":"claude","state":"paused","session":"s","cwd":"/w","activity":"Edit"}"#,
        )
        .unwrap();
        let alert = Alert::from(view);
        assert_eq!(alert.state, AgentState::Paused);
        assert_eq!(alert.message(), "fix-login is waiting for you");
        assert_eq!(alert.cwd, PathBuf::from("/w"));
    }

    #[test]
    fn alerts_chime_and_notify_unless_they_are_turned_off() {
        let waiting = [alert("a", AgentState::Paused), alert("b", AgentState::Done)];
        let on = plan(&Config::default(), &waiting);
        assert_eq!(on, Delivery { chime: Some(Chime::Waiting), notification: Some("2 agents need you".into()) });
        let done = plan(&Config::default(), &[alert("b", AgentState::Done)]);
        assert_eq!(done.chime, Some(Chime::Done));
        assert_eq!(done.notification.as_deref(), Some("b finished"));
        let silent = plan(&Config { notify_sound: false, ..Config::default() }, &waiting);
        assert_eq!(silent.chime, None, "without the chime the notification still comes");
        assert!(silent.notification.is_some());
        let off = plan(&Config { notify: false, ..Config::default() }, &waiting);
        assert_eq!(off, Delivery { chime: None, notification: None });
        assert_eq!(plan(&Config::default(), &[]), Delivery { chime: None, notification: None });
    }
}
