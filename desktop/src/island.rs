use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use orkai::fsutil;
use orkai::model::AgentId;
use orkai::util::lock;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::link::{self, Link, MAIN_WINDOW};

pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

const LABEL: &str = "island";
const PAGE: &str = "island.html";
const TITLE: &str = "Octo";
const HOVER_EVENT: &str = "island-hover";
const LAYOUT_EVENT: &str = "island-layout";
const OPEN_EVENT: &str = "island-open";
const CANVAS: Extent = Extent { width: 440.0, height: 480.0 };
const POLL: Duration = Duration::from_millis(40);
const DRAG_POLL: Duration = Duration::from_millis(16);
const DRAG_TIMEOUT: Duration = Duration::from_secs(30);
const ENTER_POLLS: u8 = 3;
const LEAVE_POLLS: u8 = 4;
const PLACE_POLLS: u32 = 50;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub width: f64,
    pub height: f64,
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
struct Region {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Region {
    fn contains(self, point: PhysicalPosition<f64>) -> bool {
        point.x >= self.x && point.x < self.x + self.width && point.y >= self.y && point.y < self.y + self.height
    }

    fn within_canvas(self) -> Self {
        let fit = |value: f64, max: f64| if value.is_finite() { value.clamp(0.0, max) } else { 0.0 };
        let x = fit(self.x, CANVAS.width);
        let y = fit(self.y, CANVAS.height);
        Self { x, y, width: fit(self.width, CANVAS.width - x), height: fit(self.height, CANVAS.height - y) }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Shape {
    collapsed: Region,
    expanded: Region,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Canvas {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}

impl Canvas {
    fn physical_size(self, coordinate_scale: f64) -> tauri::PhysicalSize<u32> {
        tauri::PhysicalSize::new(
            (self.width * coordinate_scale).round() as u32,
            (self.height * coordinate_scale).round() as u32,
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    fn vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

#[derive(Serialize, Deserialize)]
struct Dock {
    edge: Edge,
    fraction: f64,
    monitor: Option<String>,
    origin: [i32; 2],
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    edge: Edge,
    offset: f64,
    notch: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self { edge: Edge::Top, offset: CANVAS.width / 2.0, notch: true }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DragAction {
    Start,
    Drop,
    Cancel,
}

enum DragEvent {
    Start { cursor: PhysicalPosition<f64>, origin: PhysicalPosition<f64> },
    Drop(PhysicalPosition<f64>),
    Cancel,
}

struct Drag {
    cursor: PhysicalPosition<f64>,
    origin: PhysicalPosition<f64>,
    started: Instant,
}

impl Drag {
    fn active(&self, now: Instant, pressed: bool) -> bool {
        pressed && now.duration_since(self.started) < DRAG_TIMEOUT
    }
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
    layout: Mutex<Layout>,
    drag_events: Mutex<Vec<DragEvent>>,
}

impl Island {
    fn region(&self, inside: bool) -> Region {
        let shape = lock(&self.shape);
        if inside { shape.expanded } else { shape.collapsed }
    }

    fn set_layout(&self, layout: Layout) -> bool {
        let previous = std::mem::replace(&mut *lock(&self.layout), layout);
        if previous == layout {
            return false;
        }
        *lock(&self.shape) = Shape::default();
        true
    }

    fn reset_drag(&self) {
        let mut events = lock(&self.drag_events);
        events.clear();
        events.push(DragEvent::Cancel);
    }
}

fn covers(region: Region, canvas: Canvas, cursor: PhysicalPosition<f64>) -> bool {
    region.contains(PhysicalPosition::new((cursor.x - canvas.x) / canvas.scale, (cursor.y - canvas.y) / canvas.scale))
}

fn edge_at(area: Region, cursor: PhysicalPosition<f64>) -> (Edge, f64) {
    let x = (cursor.x - area.x).clamp(0.0, area.width);
    let y = (cursor.y - area.y).clamp(0.0, area.height);
    let edges = [(Edge::Top, y), (Edge::Bottom, area.height - y), (Edge::Left, x), (Edge::Right, area.width - x)];
    let (edge, _) = edges.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    let fraction = if edge.vertical() { y / area.height } else { x / area.width };
    (edge, fraction)
}

fn canvas_at(area: Region, scale: f64, edge: Edge, fraction: f64, notch: bool) -> (Canvas, Layout) {
    let width = (CANVAS.width * scale).min(area.width);
    let height = (CANVAS.height * scale).min(area.height);
    let (span, length) = if edge.vertical() { (area.height, height) } else { (area.width, width) };
    let center = span * fraction.clamp(0.0, 1.0);
    let start = (center - length / 2.0).clamp(0.0, (span - length).max(0.0)).round();
    let (x, y) = match edge {
        Edge::Top => (area.x + start, area.y),
        Edge::Bottom => (area.x + start, area.y + (area.height - height).max(0.0)),
        Edge::Left => (area.x, area.y + start),
        Edge::Right => (area.x + (area.width - width).max(0.0), area.y + start),
    };
    (Canvas { x, y, width, height, scale }, Layout { edge, offset: (center - start) / scale, notch })
}

fn canvas_for_dock(
    area: Region,
    work: Region,
    scale: f64,
    dock: Option<&Dock>,
    notch: Option<Extent>,
) -> (Canvas, Layout) {
    let (edge, fraction) = dock.map_or((Edge::Top, 0.5), |dock| (dock.edge, dock.fraction));
    let notch = dock.is_none()
        || (edge == Edge::Top && notch.is_some_and(|notch| ((fraction - 0.5) * area.width).abs() <= notch.width / 2.0));
    canvas_at(if notch { area } else { work }, scale, edge, if notch { 0.5 } else { fraction }, notch)
}

fn desktop_scale(scale: f64) -> f64 {
    if cfg!(target_os = "macos") { scale } else { 1.0 }
}

#[cfg(windows)]
fn mouse_pressed() -> bool {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    const LEFT_BUTTON: i32 = 0x01;
    unsafe { GetAsyncKeyState(LEFT_BUTTON) < 0 }
}

#[cfg(target_os = "macos")]
fn mouse_pressed() -> bool {
    mac::mouse_pressed()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn mouse_pressed() -> bool {
    true
}

fn monitor_area(monitor: &Monitor, work_area: bool) -> Region {
    let work = monitor.work_area();
    let (origin, size) = if work_area && work.size.width > 0 && work.size.height > 0 {
        (&work.position, &work.size)
    } else {
        (monitor.position(), monitor.size())
    };
    let scale = desktop_scale(monitor.scale_factor());
    Region {
        x: f64::from(origin.x) / scale,
        y: f64::from(origin.y) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
    }
}

fn position(window: &WebviewWindow) -> tauri::Result<PhysicalPosition<f64>> {
    let origin = window.outer_position()?;
    let scale = if cfg!(target_os = "macos") { window.scale_factor()? } else { 1.0 };
    Ok(PhysicalPosition::new(f64::from(origin.x) / scale, f64::from(origin.y) / scale))
}

fn move_window(window: &WebviewWindow, x: f64, y: f64) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    return window.set_position(tauri::LogicalPosition::new(x, y));
    #[cfg(not(target_os = "macos"))]
    window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}

fn cursor(window: &WebviewWindow) -> tauri::Result<PhysicalPosition<f64>> {
    let cursor = window.cursor_position()?;
    let scale =
        if cfg!(target_os = "macos") { window.primary_monitor()?.map_or(1.0, |m| m.scale_factor()) } else { 1.0 };
    Ok(PhysicalPosition::new(cursor.x / scale, cursor.y / scale))
}

fn dock_file(app: &AppHandle) -> PathBuf {
    app.state::<Link>().hub().store().home().root().join("island-position.json")
}

fn dock_on_drop(window: &WebviewWindow, cursor: PhysicalPosition<f64>) -> Option<Dock> {
    let monitor = window.available_monitors().ok()?.into_iter().find(|m| monitor_area(m, false).contains(cursor))?;
    let (edge, fraction) = edge_at(monitor_area(&monitor, true), cursor);
    Some(Dock {
        edge,
        fraction,
        monitor: monitor.name().cloned(),
        origin: [monitor.position().x, monitor.position().y],
    })
}

fn place(window: &WebviewWindow, island: &Island, dock: Option<&Dock>) -> Option<Canvas> {
    let primary = window.primary_monitor().ok()??;
    let monitor = dock
        .and_then(|dock| {
            let monitors = window.available_monitors().ok()?;
            let matches = |m: &Monitor| m.name() == dock.monitor.as_ref();
            monitors
                .iter()
                .find(|m| matches(m) && [m.position().x, m.position().y] == dock.origin)
                .or_else(|| monitors.iter().find(|m| dock.monitor.is_some() && matches(m)))
                .cloned()
        })
        .unwrap_or_else(|| primary.clone());
    let notch = (monitor.position() == primary.position()).then(island_notch).flatten();
    let scale = monitor.scale_factor() / desktop_scale(monitor.scale_factor());
    let (canvas, layout) =
        canvas_for_dock(monitor_area(&monitor, false), monitor_area(&monitor, true), scale, dock, notch);
    let spot = position(window).ok()?;
    if (spot.x - canvas.x).abs() >= 0.5 || (spot.y - canvas.y).abs() >= 0.5 {
        move_window(window, canvas.x, canvas.y).ok()?;
    }
    let wanted = canvas.physical_size(desktop_scale(monitor.scale_factor()));
    if window.inner_size().ok()? != wanted {
        window.set_size(wanted).ok()?;
    }
    let spot = position(window).ok()?;
    if (spot.x - canvas.x).abs() >= 0.5 || (spot.y - canvas.y).abs() >= 0.5 {
        move_window(window, canvas.x, canvas.y).ok()?;
    }
    if island.set_layout(layout) {
        let _ = window.emit_to(LABEL, LAYOUT_EVENT, layout);
    }
    Some(canvas)
}

fn track(app: AppHandle, epoch: u64, path: PathBuf, mut dock: Option<Dock>, mut canvas: Option<Canvas>) {
    let island = app.state::<Island>();
    let mut hover = Hover::default();
    let mut interactive = false;
    let mut polls = if canvas.is_some() { 1u32 } else { 0 };
    let mut drag: Option<Drag> = None;
    let mut last_cursor = None;
    while island.epoch.load(Ordering::SeqCst) == epoch {
        let Some(window) = app.get_webview_window(LABEL) else { break };
        let pointer = cursor(&window).ok();
        let mut events = std::mem::take(&mut *lock(&island.drag_events));
        let pressed = mouse_pressed();
        if drag.as_ref().is_some_and(|drag| !drag.active(Instant::now(), pressed)) {
            events.push(if pressed { DragEvent::Cancel } else { pointer.map_or(DragEvent::Cancel, DragEvent::Drop) });
        }
        for event in events {
            match event {
                DragEvent::Start { cursor, origin } => {
                    drag = Some(Drag { cursor, origin, started: Instant::now() });
                    last_cursor = Some(cursor);
                }
                DragEvent::Drop(_) | DragEvent::Cancel => {
                    if drag.take().is_none() {
                        continue;
                    }
                    if let DragEvent::Drop(point) = event
                        && let Some(next) = dock_on_drop(&window, point)
                    {
                        if let Err(err) = fsutil::write_json_atomic(&path, &next) {
                            eprintln!("octo: could not save the island position: {err}");
                        }
                        dock = Some(next);
                    }
                    hover = Hover::default();
                    let _ = app.emit_to(LABEL, HOVER_EVENT, false);
                    polls = 0;
                }
            }
        }
        if let Some(Drag { cursor: start, origin, .. }) = &drag {
            if let Some(point) = pointer.filter(|point| Some(*point) != last_cursor) {
                let _ = move_window(&window, origin.x + point.x - start.x, origin.y + point.y - start.y);
                last_cursor = Some(point);
            }
            thread::sleep(DRAG_POLL);
            continue;
        }
        if polls.is_multiple_of(PLACE_POLLS) {
            canvas = place(&window, &island, dock.as_ref());
        }
        polls = polls.wrapping_add(1);
        if island.region(false) == Region::default() && hover.inside {
            hover = Hover::default();
            let _ = app.emit_to(LABEL, HOVER_EVENT, false);
        }
        let over =
            canvas.zip(pointer).is_some_and(|(canvas, cursor)| covers(island.region(hover.inside), canvas, cursor));
        if let Some(inside) = hover.step(over) {
            let _ = app.emit_to(LABEL, HOVER_EVENT, inside);
        }
        let wants_pointer = over || hover.inside;
        if interactive != wants_pointer && window.set_ignore_cursor_events(!wants_pointer).is_ok() {
            interactive = wants_pointer;
        }
        thread::sleep(POLL);
    }
}

fn open(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }
    let island = app.state::<Island>();
    *lock(&island.shape) = Shape::default();
    lock(&island.drag_events).clear();
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
    let path = dock_file(app);
    let dock = fsutil::read_json_opt::<Dock>(&path).ok().flatten();
    let canvas = place(&window, &island, dock.as_ref());
    #[cfg(target_os = "macos")]
    mac::raise(&window);
    window.show()?;
    let epoch = island.epoch.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    thread::Builder::new().name("orkai-desktop-island".into()).spawn(move || track(app, epoch, path, dock, canvas))?;
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
    island.reset_drag();
    link::watch(link.shared_hub(), &island.feed, channel).map(drop)
}

#[tauri::command]
pub fn island_shape(island: State<'_, Island>, shape: Shape) {
    *lock(&island.shape) =
        Shape { collapsed: shape.collapsed.within_canvas(), expanded: shape.expanded.within_canvas() };
}

#[tauri::command]
pub fn island_layout(island: State<'_, Island>) -> Layout {
    island.reset_drag();
    *lock(&island.layout)
}

#[tauri::command]
pub fn island_drag(window: WebviewWindow, island: State<'_, Island>, action: DragAction) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("only the island can be dragged".into());
    }
    let event = match action {
        DragAction::Start => DragEvent::Start {
            cursor: cursor(&window).map_err(|err| err.to_string())?,
            origin: position(&window).map_err(|err| err.to_string())?,
        },
        DragAction::Drop => cursor(&window).map_or(DragEvent::Cancel, DragEvent::Drop),
        DragAction::Cancel => DragEvent::Cancel,
    };
    lock(&island.drag_events).push(event);
    Ok(())
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

    pub fn mouse_pressed() -> bool {
        const LEFT_BUTTON: usize = 1;
        let buttons: usize = unsafe { msg_send![class!(NSEvent), pressedMouseButtons] };
        buttons & LEFT_BUTTON != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Canvas = Canvas { x: 740.0, y: 0.0, width: 440.0, height: 480.0, scale: 1.0 };
    const PILL: Region = Region { x: 160.0, y: 0.0, width: 120.0, height: 28.0 };
    const PANEL: Region = Region { x: 50.0, y: 0.0, width: 340.0, height: 300.0 };

    fn at(x: f64, y: f64) -> PhysicalPosition<f64> {
        PhysicalPosition::new(x, y)
    }

    #[test]
    fn returning_to_the_notch_restores_the_full_screen_layout_even_with_a_saved_dock() {
        let area = Region { width: 1512.0, height: 982.0, ..Region::default() };
        let work = Region { y: 38.0, height: 944.0, ..area };
        let notch = Some(Extent { width: 184.0, height: 38.0 });
        let mut dock = Dock { edge: Edge::Top, fraction: 0.5 + 10.0 / area.width, monitor: None, origin: [0, 0] };
        let (canvas, layout) = canvas_for_dock(area, work, 1.0, Some(&dock), notch);
        assert_eq!(canvas.y, 0.0);
        assert_eq!(layout, Layout::default());
        let saved = serde_json::to_string(&dock).unwrap();
        let restored: Dock = serde_json::from_str(&saved).unwrap();
        assert_eq!(canvas_for_dock(area, work, 1.0, Some(&restored), notch), (canvas, layout));
        dock.fraction = 0.2;
        assert_eq!(canvas_for_dock(area, work, 1.0, Some(&dock), notch).0.y, work.y);
        dock.fraction = 0.5;
        for edge in [Edge::Bottom, Edge::Left, Edge::Right] {
            dock.edge = edge;
            assert!(!canvas_for_dock(area, work, 1.0, Some(&dock), notch).1.notch);
        }
        dock.edge = Edge::Top;
        assert!(!canvas_for_dock(area, work, 1.0, Some(&dock), None).1.notch);
    }

    #[test]
    fn a_layout_change_discards_the_previous_edges_mouse_regions() {
        let island = Island::default();
        let shape = Shape { collapsed: PILL, expanded: PANEL };
        *lock(&island.shape) = shape;
        assert!(!island.set_layout(Layout::default()));
        assert_eq!(*lock(&island.shape), shape);
        let layout = Layout { edge: Edge::Left, offset: 100.0, notch: false };
        assert!(island.set_layout(layout));
        assert_eq!(*lock(&island.layout), layout);
        assert_eq!(*lock(&island.shape), Shape::default());
        assert!(!covers(island.region(false), SCREEN, at(960.0, 10.0)));
    }

    #[test]
    fn releasing_the_mouse_or_reaching_the_deadline_ends_a_drag_without_a_page_event() {
        let started = Instant::now();
        let drag = Drag { cursor: at(10.0, 10.0), origin: at(0.0, 0.0), started };
        assert!(drag.active(started + DRAG_POLL, true));
        assert!(!drag.active(started + DRAG_POLL, false));
        assert!(!drag.active(started + DRAG_TIMEOUT, true));
        let island = Island::default();
        lock(&island.drag_events).push(DragEvent::Start { cursor: drag.cursor, origin: drag.origin });
        island.reset_drag();
        assert!(matches!(&lock(&island.drag_events)[..], [DragEvent::Cancel]));
    }

    #[test]
    fn the_destination_monitor_determines_the_physical_window_size() {
        let area = Region { x: 1920.0, width: 2560.0, height: 1440.0, ..Region::default() };
        let (canvas, _) = canvas_at(area, 1.5, Edge::Right, 0.5, false);
        assert_eq!(canvas.physical_size(1.0), tauri::PhysicalSize::new(660, 720));
        assert_eq!(canvas.x, 3820.0);
        let (retina, _) = canvas_at(area, 1.0, Edge::Top, 0.5, true);
        assert_eq!(retina.physical_size(2.0), tauri::PhysicalSize::new(880, 960));
    }

    #[test]
    fn dropping_on_another_monitor_preserves_the_point_along_its_nearest_edge() {
        let area = Region { x: -1600.0, y: 120.0, width: 1600.0, height: 900.0 };
        assert_eq!(edge_at(area, at(-1200.0, 129.0)), (Edge::Top, 0.25));
        assert_eq!(edge_at(area, at(-1200.0, 1015.0)), (Edge::Bottom, 0.25));
        assert_eq!(edge_at(area, at(-1592.0, 345.0)), (Edge::Left, 0.25));
        assert_eq!(edge_at(area, at(-7.0, 345.0)), (Edge::Right, 0.25));

        let (canvas, layout) = canvas_at(area, 1.5, Edge::Right, 0.25, false);
        assert_eq!((canvas.x, canvas.y), (-660.0, 120.0));
        assert_eq!(layout, Layout { edge: Edge::Right, offset: 150.0, notch: false });
        let pill = Region { x: 320.0, y: 136.0, ..PILL };
        assert!(covers(pill, canvas, at(-7.0, 345.0)));
        assert!(!covers(pill, canvas, at(-181.0, 345.0)), "the transparent area still lets the mouse through");
    }

    #[test]
    fn the_island_hangs_from_the_top_centre_of_the_main_screen() {
        let main = Region { width: 1920.0, height: 1080.0, ..Region::default() };
        assert_eq!(canvas_at(main, 1.0, Edge::Top, 0.5, true), (SCREEN, Layout::default()));
        let small = Region { width: 400.0, height: 350.0, ..main };
        let (canvas, layout) = canvas_at(small, 1.0, Edge::Bottom, 0.5, false);
        assert_eq!((canvas.x, canvas.y, canvas.width, canvas.height), (0.0, 0.0, 400.0, 350.0));
        assert_eq!(layout.offset, 200.0);
    }

    #[test]
    fn only_the_pill_itself_catches_the_mouse_and_the_rest_of_the_window_lets_it_through() {
        assert!(covers(PILL, SCREEN, at(960.0, 0.0)), "the very top edge of the screen opens it");
        assert!(covers(PILL, SCREEN, at(900.0, 27.0)) && covers(PILL, SCREEN, at(1019.0, 27.0)));
        assert!(!covers(PILL, SCREEN, at(899.0, 10.0)) && !covers(PILL, SCREEN, at(1020.0, 10.0)));
        assert!(!covers(PILL, SCREEN, at(960.0, 28.0)), "what sits under the pill stays clickable");
        assert!(!covers(PILL, SCREEN, at(760.0, 10.0)), "the transparent rest of the window is not the island");
        assert!(covers(PANEL, SCREEN, at(800.0, 299.0)), "once open the whole panel holds it open");
        assert!(!covers(Region::default(), SCREEN, at(960.0, 0.0)), "nothing is caught before the page says its size");
    }

    #[test]
    fn the_region_follows_the_scale_and_the_place_of_the_screen() {
        let retina = Canvas { x: -1610.0, y: 120.0, width: 660.0, height: 720.0, scale: 1.5 };
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
        let huge = Region { x: 150.0, y: f64::NAN, width: 9000.0, height: f64::INFINITY };
        assert_eq!(huge.within_canvas(), Region { x: 150.0, y: 0.0, width: 290.0, height: 0.0 });
        assert_eq!(PANEL.within_canvas(), PANEL);
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
            serde_json::from_str(r#"{"collapsed":{"x":160,"y":0,"width":120,"height":28},"expanded":{"x":50,"y":0,"width":340,"height":300}}"#)
                .unwrap();
        assert_eq!(shape, Shape { collapsed: PILL, expanded: PANEL });
    }
}
