use tauri::{App, LogicalPosition, WebviewWindowBuilder};

const SYSTEM_VERSION: &str = "/System/Library/CoreServices/SystemVersion.plist";
const PRODUCT_VERSION_KEY: &str = "<key>ProductVersion</key>";
const TAHOE: u32 = 26;
const TRAFFIC_LIGHTS_X: f64 = 16.0;
const TRAFFIC_LIGHTS_Y: f64 = 24.0;
const TRAFFIC_LIGHTS_Y_BEFORE_TAHOE: f64 = 20.0;

pub fn open(app: &App) -> tauri::Result<()> {
    let Some(config) = app.config().app.windows.first() else { return Ok(()) };
    let version = std::fs::read_to_string(SYSTEM_VERSION).ok().and_then(|plist| major_version(&plist));
    WebviewWindowBuilder::from_config(app.handle(), config)?
        .traffic_light_position(LogicalPosition::new(TRAFFIC_LIGHTS_X, traffic_lights_y(version)))
        .build()?;
    Ok(())
}

fn major_version(plist: &str) -> Option<u32> {
    let (_, after_key) = plist.split_once(PRODUCT_VERSION_KEY)?;
    let (_, value) = after_key.split_once("<string>")?;
    value.split(['.', '<']).next()?.trim().parse().ok()
}

fn traffic_lights_y(major: Option<u32>) -> f64 {
    if major.is_some_and(|major| major < TAHOE) { TRAFFIC_LIGHTS_Y_BEFORE_TAHOE } else { TRAFFIC_LIGHTS_Y }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist(version: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\t<key>ProductBuildVersion</key>\n\t<string>25G83</string>\n\t<key>ProductName</key>\n\t<string>macOS</string>\n\t<key>ProductVersion</key>\n\t<string>{version}</string>\n</dict>\n</plist>\n"
        )
    }

    #[test]
    fn the_major_macos_version_is_read_from_the_system_version_file() {
        assert_eq!(major_version(&plist("26.6.2")), Some(26));
        assert_eq!(major_version(&plist("15.6")), Some(15));
        assert_eq!(major_version(&plist("27")), Some(27));
        assert_eq!(major_version("<plist><dict></dict></plist>"), None);
        assert_eq!(major_version(&plist("unknown")), None);
    }

    #[test]
    fn the_traffic_lights_sit_lower_before_tahoe_drew_them_bigger() {
        assert_eq!(traffic_lights_y(Some(15)), TRAFFIC_LIGHTS_Y_BEFORE_TAHOE);
        assert_eq!(traffic_lights_y(Some(26)), TRAFFIC_LIGHTS_Y);
        assert_eq!(traffic_lights_y(Some(27)), TRAFFIC_LIGHTS_Y);
        assert_eq!(traffic_lights_y(None), TRAFFIC_LIGHTS_Y, "an unreadable version gets the current design");
    }

    #[test]
    fn this_mac_reports_its_version() {
        let plist = std::fs::read_to_string(SYSTEM_VERSION).unwrap();
        assert!(major_version(&plist).is_some_and(|major| major >= 11), "{plist}");
    }
}
