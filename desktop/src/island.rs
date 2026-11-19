use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use orkai::model::AgentId;
use orkai::util::lock;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::link::{self, Link, MAIN_WINDOW};

pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

const LABEL: &str = "island";
const PAGE: &str = "island.html";
const TITLE: &str = "Octo";
const HOVER_EVENT: &str = "island-hover";
const OPEN_EVENT: &str = "island-open";
const CANVAS: Extent = Extent { width: 440.0, height: 480.0 };
const POLL: Duration = Duration::from_millis(40);
const ENTER_POLLS: u8 = 3;
const LEAVE_POLLS: u8 = 4;
const PLACE_POLLS: u32 = 50;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Extent {
    pub width: f64,
    pub height: f64,
}

impl Extent {
    fn within(self, limit: Extent) -> Extent {
        let fit = |value: f64, max: f64| if value.is_finite() { value.clamp(0.0, max) } else { 0.0 };
        Extent { width: fit(self.width, limit.width), height: fit(self.height, limit.height) }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Shape {
    pub collapsed: Extent,
    pub expanded: Extent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Canvas {
    x: f64,
    y: f64,
    width: f64,
    scale: f64,
}

#[derive(Default)]
struct Hover {
    inside: bool,
    streak: u8,
}

impl Hover {
    fn step(&mut self, over: bool) -> Option<bool> {
        if over == self.inside {
            self.streak = 0;
            return None;
        }
        self.streak += 1;
        if self.streak < if over { ENTER_POLLS } else { LEAVE_POLLS } {
            return None;
        }
        self.inside = over;
        self.streak = 0;
        Some(over)
    }
}

#[derive(Default)]
pub struct Island {
    feed: Arc<AtomicU64>,
    epoch: AtomicU64,
    shape: Mutex<Shape>,
}

impl Island {
    fn region(&self, inside: bool) -> Extent {
        let shape = lock(&self.shape);
        if inside { shape.expanded } else { shape.collapsed }
    }
}

fn covers(region: Extent, canvas: Canvas, cursor: PhysicalPosition<f64>) -> bool {
    let half = region.width * canvas.scale / 2.0;
    let center = canvas.x + canvas.width / 2.0;
    let bottom = canvas.y + region.height * canvas.scale;
    cursor.x >= center - half && cursor.x < center + half && cursor.y >= canvas.y && cursor.y < bottom
}

fn anchor(monitor: PhysicalPosition<i32>, monitor_width: u32, window_width: u32) -> PhysicalPosition<i32> {
    let spare = i64::from(monitor_width) - i64::from(window_width);
    PhysicalPosition::new(monitor.x + (spare / 2) as i32, monitor.y)
}

fn place(window: &WebviewWindow) -> Option<Canvas> {
    let monitor = window.primary_monitor().ok()??;
    let size = window.outer_size().ok()?;
    let spot = anchor(*monitor.position(), monitor.size().width, size.width);
    if window.outer_position().ok()? != spot {
        window.set_position(spot).ok()?;
    }
    let scale = window.scale_factor().ok()?;
    Some(Canvas { x: f64::from(spot.x), y: f64::from(spot.y), width: f64::from(size.width), scale })
}

fn track(app: AppHandle, epoch: u64) {
    let island = app.state::<Island>();
    let mut hover = Hover::default();
    let mut canvas = None;
    let mut polls = 0u32;
    while island.epoch.load(Ordering::SeqCst) == epoch {
        let Some(window) = app.get_webview_window(LABEL) else { break };
        if polls.is_multiple_of(PLACE_POLLS) {
            canvas = place(&window);
        }
        polls = polls.wrapping_add(1);
        let over = canvas
            .zip(app.cursor_position().ok())
            .is_some_and(|(canvas, cursor)| covers(island.region(hover.inside), canvas, cursor));
        if let Some(inside) = hover.step(over) {
            let _ = window.set_ignore_cursor_events(!inside);
            let _ = app.emit_to(LABEL, HOVER_EVENT, inside);
        }
        thread::sleep(POLL);
    }
}

fn open(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(PAGE.into()))
        .title(TITLE)
        .inner_size(CANVAS.width, CANVAS.height)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .focusable(false)
        .accept_first_mouse(true)
        .visible(false)
        .build()?;
    window.set_ignore_cursor_events(true)?;
    place(&window);
    #[cfg(target_os = "macos")]
    mac::raise(&window);
    window.show()?;
    let epoch = app.state::<Island>().epoch.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    thread::Builder::new().name("orkai-desktop-island".into()).spawn(move || track(app, epoch))?;
    Ok(())
}

fn report(result: tauri::Result<()>) {
    if let Err(err) = result {
        eprintln!("octo: the island could not open: {err}");
    }
}

pub fn close(app: &AppHandle) {
    let island = app.state::<Island>();
    island.epoch.fetch_add(1, Ordering::SeqCst);
    island.feed.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
}

pub fn on_launch(app: &AppHandle) {
    if SUPPORTED && app.state::<Link>().hub().launcher().config().island {
        report(open(app));
    }
}

pub fn apply(app: &AppHandle, on: bool) {
    if !SUPPORTED {
        return;
    }
    if !on {
        close(app);
        return;
    }
    let app = app.clone();
    let _ = thread::Builder::new().name("orkai-desktop-island".into()).spawn(move || report(open(&app)));
}

#[cfg(any(target_os = "macos", test))]
fn notch_of(top_inset: f64, screen_width: f64, left_width: f64, right_width: f64) -> Option<Extent> {
    let width = screen_width - left_width - right_width;
    (top_inset > 0.0 && left_width > 0.0 && right_width > 0.0 && width > 0.0)
        .then_some(Extent { width, height: top_inset })
}

#[tauri::command]
pub fn island_attach(link: State<'_, Link>, island: State<'_, Island>, channel: Channel) -> Result<(), String> {
    link::watch(link.shared_hub(), &island.feed, channel).map(drop)
}

#[tauri::command]
pub fn island_shape(island: State<'_, Island>, shape: Shape) {
    *lock(&island.shape) = Shape { collapsed: shape.collapsed.within(CANVAS), expanded: shape.expanded.within(CANVAS) };
}

#[tauri::command]
pub fn island_notch() -> Option<Extent> {
    #[cfg(target_os = "macos")]
    return mac::notch();
    #[cfg(not(target_os = "macos"))]
    None
}

#[tauri::command]
pub fn island_open(app: AppHandle, agent: Option<AgentId>) -> Result<(), String> {
    let main = app.get_webview_window(MAIN_WINDOW).ok_or_else(|| "the Octo window is gone".to_string())?;
    main.unminimize().and_then(|()| main.show()).and_then(|()| main.set_focus()).map_err(|err| err.to_string())?;
    match agent {
        Some(agent) => app.emit_to(MAIN_WINDOW, OPEN_EVENT, agent).map_err(|err| err.to_string()),
        None => Ok(()),
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::encode::{Encode, Encoding};
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send, sel};
    use tauri::WebviewWindow;

    use super::Extent;

    const ABOVE_THE_MENU_BAR: isize = 25;
    const JOINS_ALL_SPACES: usize = 1 << 0;
    const STATIONARY: usize = 1 << 4;
    const IGNORES_CYCLE: usize = 1 << 6;
    const FULL_SCREEN_AUXILIARY: usize = 1 << 8;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        origin: Point,
        size: Size,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Insets {
        top: f64,
        left: f64,
        bottom: f64,
        right: f64,
    }

    unsafe impl Encode for Point {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }

    unsafe impl Encode for Size {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }

    unsafe impl Encode for Rect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
    }

    unsafe impl Encode for Insets {
        const ENCODING: Encoding =
            Encoding::Struct("NSEdgeInsets", &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING]);
    }

    pub fn raise(window: &WebviewWindow) {
        let handle = window.clone();
        let _ = window.run_on_main_thread(move || {
            let Ok(ns_window) = handle.ns_window() else { return };
            let ns_window = ns_window.cast::<AnyObject>();
            let behavior = JOINS_ALL_SPACES | STATIONARY | IGNORES_CYCLE | FULL_SCREEN_AUXILIARY;
            unsafe {
                let _: () = msg_send![ns_window, setLevel: ABOVE_THE_MENU_BAR];
                let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
            }
        });
    }

    pub fn notch() -> Option<Extent> {
        unsafe {
            let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
            if screens.is_null() {
                return None;
            }
            let count: usize = msg_send![screens, count];
            if count == 0 {
                return None;
            }
            let screen: *mut AnyObject = msg_send![screens, objectAtIndex: 0usize];
            let knows: Bool = msg_send![screen, respondsToSelector: sel!(safeAreaInsets)];
            if !knows.as_bool() {
                return None;
            }
            let insets: Insets = msg_send![screen, safeAreaInsets];
            let frame: Rect = msg_send![screen, frame];
            let left: Rect = msg_send![screen, auxiliaryTopLeftArea];
            let right: Rect = msg_send![screen, auxiliaryTopRightArea];
            super::notch_of(insets.top, frame.size.width, left.size.width, right.size.width)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Canvas = Canvas { x: 750.0, y: 0.0, width: 420.0, scale: 1.0 };
    const PILL: Extent = Extent { width: 120.0, height: 28.0 };
    const PANEL: Extent = Extent { width: 340.0, height: 300.0 };

    fn at(x: f64, y: f64) -> PhysicalPosition<f64> {
        PhysicalPosition::new(x, y)
    }

    #[test]
    fn the_island_hangs_from_the_top_centre_of_the_main_screen() {
        assert_eq!(anchor(PhysicalPosition::new(0, 0), 1920, 420), PhysicalPosition::new(750, 0));
        assert_eq!(anchor(PhysicalPosition::new(-2560, 120), 2560, 630), PhysicalPosition::new(-1595, 120));
        assert_eq!(anchor(PhysicalPosition::new(0, 0), 400, 420), PhysicalPosition::new(-10, 0), "a narrow screen");
    }

    #[test]
    fn only_the_pill_itself_catches_the_mouse_and_the_rest_of_the_window_lets_it_through() {
        assert!(covers(PILL, SCREEN, at(960.0, 0.0)), "the very top edge of the screen opens it");
        assert!(covers(PILL, SCREEN, at(900.0, 27.0)) && covers(PILL, SCREEN, at(1019.0, 27.0)));
        assert!(!covers(PILL, SCREEN, at(899.0, 10.0)) && !covers(PILL, SCREEN, at(1020.0, 10.0)));
        assert!(!covers(PILL, SCREEN, at(960.0, 28.0)), "what sits under the pill stays clickable");
        assert!(!covers(PILL, SCREEN, at(760.0, 10.0)), "the transparent rest of the window is not the island");
        assert!(covers(PANEL, SCREEN, at(800.0, 299.0)), "once open the whole panel holds it open");
        assert!(!covers(Extent::default(), SCREEN, at(960.0, 0.0)), "nothing is caught before the page says its size");
    }

    #[test]
    fn the_region_follows_the_scale_and_the_place_of_the_screen() {
        let retina = Canvas { x: -1595.0, y: 120.0, width: 630.0, scale: 1.5 };
        assert!(covers(PILL, retina, at(-1280.0, 120.0)));
        assert!(covers(PILL, retina, at(-1369.0, 161.0)) && !covers(PILL, retina, at(-1371.0, 161.0)));
        assert!(!covers(PILL, retina, at(-1280.0, 162.0)) && !covers(PILL, retina, at(-1280.0, 119.0)));
    }

    #[test]
    fn a_mouse_passing_by_does_not_open_it_and_a_slip_does_not_close_it() {
        let mut hover = Hover::default();
        assert_eq!([true, true, false, true, true].map(|over| hover.step(over)), [None; 5], "it only passed by");
        assert_eq!(hover.step(true), Some(true), "resting on it opens it");
        assert_eq!([true, false, false, false, true].map(|over| hover.step(over)), [None; 5], "a slip keeps it open");
        assert_eq!([false, false, false].map(|over| hover.step(over)), [None; 3]);
        assert_eq!(hover.step(false), Some(false), "leaving closes it");
        assert_eq!(hover.step(false), None, "and it says so once");
    }

    #[test]
    fn the_page_cannot_claim_more_of_the_screen_than_the_window_has() {
        let huge = Extent { width: 9000.0, height: f64::INFINITY };
        assert_eq!(huge.within(CANVAS), Extent { width: CANVAS.width, height: 0.0 });
        assert_eq!(Extent { width: -5.0, height: f64::NAN }.within(CANVAS), Extent::default());
        assert_eq!(PANEL.within(CANVAS), PANEL);
    }

    #[test]
    fn a_notch_is_what_the_screen_hides_between_its_two_top_areas() {
        assert_eq!(notch_of(38.0, 1512.0, 664.0, 664.0), Some(Extent { width: 184.0, height: 38.0 }));
        assert_eq!(notch_of(0.0, 1920.0, 0.0, 0.0), None, "a screen without a notch");
        assert_eq!(notch_of(38.0, 1512.0, 0.0, 0.0), None, "an inset without the two areas is not a notch");
    }

    #[test]
    fn the_shape_the_page_sends_is_read_as_it_is_written() {
        let shape: Shape =
            serde_json::from_str(r#"{"collapsed":{"width":120,"height":28},"expanded":{"width":340,"height":300}}"#)
                .unwrap();
        assert_eq!(shape, Shape { collapsed: PILL, expanded: PANEL });
    }
}
