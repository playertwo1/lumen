// The pill of Flow dictation: a small see-through capsule at the bottom of the
// screen (or on its left or right edge) while dictation is on. It opens into
// Mochi and nine dots while you speak, into a toolbar under the pointer, and
// shows a call being recorded and short messages. Port of FlowPill.swift.
//
// Its window is borderless, transparent, always on top, never takes the
// keyboard, and lets clicks through except where the pill and its bubble are:
// a cursor poll on Windows (like the island), the input region on Linux.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use super::store::Edge;
use crate::platform::{self, DesktopMode};

pub const LABEL: &str = "pill";

/// Window size (logical) along the bottom, and on the sides.
const BOTTOM: (f64, f64) = (520.0, 170.0);
const SIDE: (f64, f64) = (480.0, 320.0);
/// Off-screen spot on Windows while hidden: the webview keeps running, ready.
const PARKED: (i32, i32) = (-32000, -32000);

/// What the pill shows, sent whole to the page on every change.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PillState {
    /// "idle", "listening", "thinking", "locked", "copied", "failed", "pastedAgain".
    pub phase: String,
    pub edge: String,
    /// Where along its edge the pill sits (0..1), after a drag; 0.5 is the middle.
    pub offset: f64,
    /// The dictation key, as the page writes it ("right_ctrl"…).
    pub key: String,
    /// When the call being recorded started (ms since 1970), or none.
    pub note_started_at: Option<u64>,
    pub announcement: Option<Announcement>,
    /// Dictation is on (the resting pill shows).
    pub enabled: bool,
    /// The user chose to hide the resting pill: it shows only while something happens.
    pub hide_idle: bool,
    /// « OK Coucou » is talking with you.
    pub voice: Option<crate::voice::VoiceView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    pub id: u64,
    /// "mic", "lock", "clipboard", "download", "check", "error".
    pub icon: String,
    pub tint: String,
    pub text: String,
    /// The button's label, if there is one.
    pub action: Option<String>,
    /// What the button does: "openSettings", "openNotes".
    pub action_id: Option<String>,
}

pub struct Pill {
    mode: DesktopMode,
    state: Mutex<PillState>,
    /// Where the page says the pill and its bubble are (window-logical rects).
    hot: Mutex<Vec<(f64, f64, f64, f64)>>,
    shown: AtomicBool,
    /// The pointer is on the pill or its bubble: a message waits for it to leave.
    pub hovered: AtomicBool,
    #[cfg(target_os = "linux")]
    pointer_watch: AtomicBool,
    /// Parks the Windows cursor poll while the pill is hidden.
    poll: (Mutex<bool>, Condvar),
    announce_seq: std::sync::atomic::AtomicU64,
}

impl Pill {
    pub fn state(&self) -> PillState {
        self.state.lock().unwrap().clone()
    }
}

fn page_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/pill.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("pill.html".into())
}

/// Creates the pill's window, hidden. Before the island, for WebView2's sake
/// (see create_settings_window in lib.rs).
pub fn setup(app: &AppHandle) {
    let mut mode = platform::desktop_mode();
    if mode == DesktopMode::Off {
        crate::log::line("flow pill: no way to place it on this desktop");
    } else {
        let park = cfg!(windows);
        let mut builder = WebviewWindowBuilder::new(app, LABEL, page_url(app))
            .additional_browser_args(crate::BROWSER_ARGS)
            .title(crate::PRODUCT_NAME)
            .inner_size(BOTTOM.0, BOTTOM.1)
            .resizable(cfg!(target_os = "linux"))
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible_on_all_workspaces(true)
            .focused(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .disable_drag_drop_handler()
            .focusable(false)
            .visible(park);
        if park {
            builder = builder.position(PARKED.0 as f64, PARKED.1 as f64);
        }
        match builder.build() {
            Ok(win) => {
                // Windows: prepare_pill_window also makes the idle window
                // click-through. Not here for every system: on Linux the
                // window is not realized yet, and tao unwraps its GdkWindow.
                if !platform::prepare_pill_window(&win, mode) {
                    let _ = win.destroy();
                    mode = DesktopMode::Off;
                } else if park {
                    let _ = win.set_position(PhysicalPosition::new(PARKED.0, PARKED.1));
                }
            }
            Err(err) => {
                crate::log::line(format!("flow pill window failed: {err}"));
                mode = DesktopMode::Off;
            }
        }
    }
    let pill = Arc::new(Pill {
        mode,
        state: Mutex::new(PillState { phase: "idle".into(), edge: "bottom".into(), offset: 0.5, ..Default::default() }),
        hot: Mutex::new(Vec::new()),
        shown: AtomicBool::new(false),
        hovered: AtomicBool::new(false),
        #[cfg(target_os = "linux")]
        pointer_watch: AtomicBool::new(false),
        poll: (Mutex::new(false), Condvar::new()),
        announce_seq: std::sync::atomic::AtomicU64::new(1),
    });
    app.manage(pill.clone());
    show(app, &pill, false);
    if mode == DesktopMode::Poll {
        spawn_poll(app.clone(), pill);
    }
}

/// A dictation is under way: the microphone is Flow's.
pub fn dictating(app: &AppHandle) -> bool {
    get(app).is_some_and(|p| matches!(p.state.lock().unwrap().phase.as_str(), "listening" | "locked" | "thinking"))
}

pub fn get(app: &AppHandle) -> Option<Arc<Pill>> {
    app.try_state::<Arc<Pill>>().map(|s| s.inner().clone())
}

/// WebKitGTK can miss pointerleave when the window's input region changes.
/// Check GTK's pointer focus while hovered, as the island does, and park the
/// timer as soon as the page folds the toolbar or the pill is hidden.
#[cfg(target_os = "linux")]
pub fn watch_pointer_leave(app: &AppHandle) {
    use gtk::prelude::*;

    let Some(pill) = get(app) else { return };
    if pill.pointer_watch.swap(true, Ordering::Relaxed) {
        return;
    }
    let app_for_watch = app.clone();
    let watched = pill.clone();
    if app.run_on_main_thread(move || {
        let Some(win) = window(&app_for_watch) else {
            watched.pointer_watch.store(false, Ordering::Relaxed);
            return;
        };
        let Ok(gw) = win.gtk_window() else {
            watched.pointer_watch.store(false, Ordering::Relaxed);
            return;
        };
        gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
            if !watched.shown.load(Ordering::Relaxed)
                || !watched.hovered.load(Ordering::Relaxed)
                || !gw.is_visible()
            {
                watched.pointer_watch.store(false, Ordering::Relaxed);
                return gtk::glib::ControlFlow::Break;
            }
            if let (Some(ours), Some(pointer)) = (
                gw.window(),
                gtk::gdk::Display::default().and_then(|d| d.default_seat()).and_then(|s| s.pointer()),
            ) {
                let (under, _, _) = pointer.window_at_position();
                if !under.is_some_and(|w| w.toplevel() == ours.toplevel()) {
                    let _ = win.emit("pill-pointer", false);
                }
            }
            gtk::glib::ControlFlow::Continue
        });
    }).is_err() {
        pill.pointer_watch.store(false, Ordering::Relaxed);
    }
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

fn size_for(edge: &str) -> (f64, f64) {
    if edge == "left" || edge == "right" {
        SIDE
    } else {
        BOTTOM
    }
}

pub fn edge_name(edge: Edge) -> &'static str {
    match edge {
        Edge::Bottom => "bottom",
        Edge::BottomLeft => "bottom-left",
        Edge::BottomRight => "bottom-right",
        Edge::Left => "left",
        Edge::Right => "right",
        Edge::Top => "top",
        Edge::Hidden => "hidden",
    }
}

/// Where a pill let go at `(px, py)` sticks: the nearest edge of the work area
/// `(ax, ay, aw, ah)`, and how far along that edge (0..1). Never in the middle of the screen.
pub fn snap(px: f64, py: f64, ax: f64, ay: f64, aw: f64, ah: f64) -> (Edge, f64) {
    let along_x = ((px - ax) / aw.max(1.0)).clamp(0.0, 1.0);
    let along_y = ((py - ay) / ah.max(1.0)).clamp(0.0, 1.0);
    let sides = [
        (px - ax, Edge::Left, along_y),
        (ax + aw - px, Edge::Right, along_y),
        (py - ay, Edge::Top, along_x),
        (ay + ah - py, Edge::Bottom, along_x),
    ];
    let (_, edge, along) = sides.into_iter().fold(sides[3], |best, s| if s.0 < best.0 { s } else { best });
    (edge, along)
}

static DRAGGING: AtomicBool = AtomicBool::new(false);

/// The pill follows the pointer, held where it was grabbed (window-logical px).
pub fn drag_start(app: &AppHandle, grab_x: f64, grab_y: f64) {
    let Some(pill) = get(app) else { return };
    // A layer surface (Linux Wayland) is placed by the compositor: no free drag there.
    if pill.mode == DesktopMode::Layer || pill.mode == DesktopMode::Off || DRAGGING.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        while DRAGGING.load(Ordering::Acquire) {
            if let (Some(win), Some((cx, cy))) = (window(&app), platform::cursor_physical()) {
                let scale = win.scale_factor().unwrap_or(1.0);
                let _ = win.set_position(PhysicalPosition::new((cx - grab_x * scale) as i32, (cy - grab_y * scale) as i32));
            }
            std::thread::sleep(Duration::from_millis(12));
        }
    });
}

/// The pill is let go: it sticks to the edge nearest the pointer. Returns the new edge and offset.
pub fn drag_end(app: &AppHandle) -> Option<(Edge, f64)> {
    if !DRAGGING.swap(false, Ordering::AcqRel) {
        return None;
    }
    let (cx, cy) = platform::cursor_physical()?;
    let m = monitor(app, true)?;
    let wa = m.work_area();
    Some(snap(cx, cy, wa.position.x as f64, wa.position.y as f64, wa.size.width as f64, wa.size.height as f64))
}

/// Changes what the pill shows, and shows or hides it.
pub fn update(app: &AppHandle, change: impl FnOnce(&mut PillState)) {
    let Some(pill) = get(app) else { return };
    let (state, edge_changed) = {
        let mut s = pill.state.lock().unwrap();
        let before = (s.edge.clone(), s.offset);
        change(&mut s);
        (s.clone(), (s.edge.clone(), s.offset) != before)
    };
    let _ = app.emit_to(LABEL, "pill-state", &state);
    let visible = state.edge != "hidden"
        && ((state.enabled && !state.hide_idle) || state.note_started_at.is_some() || state.announcement.is_some() || state.voice.is_some() || state.phase != "idle");
    let was = pill.shown.swap(visible, Ordering::Relaxed);
    if visible && (!was || edge_changed) {
        place(app, &pill, &state.edge, state.offset, state.phase == "listening");
    }
    if visible != was {
        show(app, &pill, visible);
    }
}

fn show(app: &AppHandle, pill: &Pill, visible: bool) {
    let Some(win) = window(app) else { return };
    crate::webview_memory::set_low(&win, !visible);
    match pill.mode {
        DesktopMode::Poll => {
            if !visible {
                let _ = win.set_position(PhysicalPosition::new(PARKED.0, PARKED.1));
                let _ = win.set_ignore_cursor_events(true);
            }
            let (lock, cv) = &pill.poll;
            *lock.lock().unwrap() = visible;
            cv.notify_all();
        }
        DesktopMode::Window | DesktopMode::Layer => {
            let w = win.clone();
            let _ = app.run_on_main_thread(move || {
                if visible {
                    let _ = w.show();
                    let _ = w.set_always_on_top(true);
                } else {
                    let _ = w.hide();
                }
            });
        }
        DesktopMode::Off => {}
    }
    let _ = app.emit_to(LABEL, "pill-visible", visible);
}

/// The display to sit on: the one under the cursor when you speak, else the
/// primary one.
fn monitor(app: &AppHandle, on_cursor: bool) -> Option<tauri::Monitor> {
    if on_cursor {
        if let Some((cx, cy)) = platform::cursor_physical() {
            if let Ok(list) = app.available_monitors() {
                if let Some(m) = list.into_iter().find(|m| {
                    let (p, s) = (m.position(), m.size());
                    cx >= p.x as f64 && cx < (p.x + s.width as i32) as f64 && cy >= p.y as f64 && cy < (p.y + s.height as i32) as f64
                }) {
                    return Some(m);
                }
            }
        }
    }
    app.primary_monitor().ok().flatten()
}

/// The middle of the pill along its edge, `offset` (0..1) of the way, kept `margin`
/// from the corners so its bubble stays on screen.
fn along(start: i32, length: i32, offset: f64, margin: i32) -> i32 {
    let c = start + (offset.clamp(0.0, 1.0) * length as f64).round() as i32;
    c.clamp(start + margin.min(length / 2), start + length - margin.min(length / 2))
}

fn place(app: &AppHandle, pill: &Pill, edge: &str, offset: f64, on_cursor: bool) {
    let Some(win) = window(app) else { return };
    let (lw, lh) = size_for(edge);
    match pill.mode {
        DesktopMode::Poll | DesktopMode::Window => {
            let Some(m) = monitor(app, on_cursor) else { return };
            let scale = m.scale_factor();
            let wa = m.work_area();
            let (pw, ph) = ((lw * scale).round() as i32, (lh * scale).round() as i32);
            let (ax, ay, aw, ah) = (wa.position.x, wa.position.y, wa.size.width as i32, wa.size.height as i32);
            let gap = (4.0 * scale).round() as i32;
            let margin = (130.0 * scale).round() as i32;
            // The window may reach past the screen's edge: only the pill and its bubble show.
            let (x, y) = match edge {
                "left" => (ax + gap, along(ay, ah, offset, margin) - ph / 2),
                "right" => (ax + aw - pw - gap, along(ay, ah, offset, margin) - ph / 2),
                "top" => (along(ax, aw, offset, margin) - pw / 2, ay + gap),
                "bottom-left" => (ax + gap, ay + ah - ph - gap),
                "bottom-right" => (ax + aw - pw - gap, ay + ah - ph - gap),
                _ => (along(ax, aw, offset, margin) - pw / 2, ay + ah - ph - gap),
            };
            #[cfg(target_os = "linux")]
            let _ = win.set_resizable(true);
            let _ = win.set_size(PhysicalSize::new(pw as u32, ph as u32));
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
        DesktopMode::Layer => {
            let edge = edge.to_string();
            let w = win.clone();
            let _ = app.run_on_main_thread(move || {
                platform::set_pill_edge(&w, &edge);
                let _ = w.set_size(tauri::LogicalSize::new(lw, lh));
            });
        }
        DesktopMode::Off => {}
    }
}

/// Puts the pill back on its edge (after a drag that ended where it already was).
pub fn reposition(app: &AppHandle) {
    let Some(pill) = get(app) else { return };
    let s = pill.state();
    if pill.shown.load(Ordering::Relaxed) {
        place(app, &pill, &s.edge, s.offset, false);
    }
}

/// The page tells where the pill and its bubble are, in window-logical pixels.
pub fn set_hot(app: &AppHandle, rects: Vec<(f64, f64, f64, f64)>) {
    let Some(pill) = get(app) else { return };
    *pill.hot.lock().unwrap() = rects.clone();
    if pill.mode != DesktopMode::Poll {
        if let Some(win) = window(app) {
            let _ = app.run_on_main_thread(move || platform::set_pill_input(&win, rects));
        }
    }
}

/// Windows: ~30 times a second while the pill shows, the window takes the
/// mouse only over the pill and its bubble.
fn spawn_poll(app: AppHandle, pill: Arc<Pill>) {
    std::thread::spawn(move || {
        let mut ignoring: Option<bool> = Some(true);
        loop {
            {
                let (lock, cv) = &pill.poll;
                let mut on = lock.lock().unwrap();
                while !*on {
                    if ignoring != Some(true) {
                        ignoring = Some(true);
                        if let Some(win) = window(&app) {
                            let _ = win.set_ignore_cursor_events(true);
                        }
                    }
                    on = cv.wait(on).unwrap();
                }
            }
            std::thread::sleep(Duration::from_millis(33));
            let Some(win) = window(&app) else { continue };
            let (Ok(origin), Some((cx, cy))) = (win.outer_position(), platform::cursor_physical()) else { continue };
            let scale = win.scale_factor().unwrap_or(1.0);
            let (x, y) = ((cx - origin.x as f64) / scale, (cy - origin.y as f64) / scale);
            let over = pill.hot.lock().unwrap().iter().any(|&(rx, ry, rw, rh)| x >= rx && x <= rx + rw && y >= ry && y <= ry + rh);
            if ignoring != Some(!over) {
                ignoring = Some(!over);
                let _ = win.set_ignore_cursor_events(!over);
                let _ = win.emit("pill-pointer", over);
            }
        }
    });
}

/// A message next to the pill, with a button to act; it goes away by itself
/// after a while unless the pointer is on it (the page keeps it then).
pub fn announce(app: &AppHandle, icon: &str, tint: &str, text: String, action: Option<(String, &str)>, seconds: u64) {
    let Some(pill) = get(app) else { return };
    let id = pill.announce_seq.fetch_add(1, Ordering::Relaxed);
    let (label, action_id) = match action {
        Some((label, id)) => (Some(label), Some(id.to_string())),
        None => (None, None),
    };
    let a = Announcement { id, icon: icon.into(), tint: tint.into(), text, action: label, action_id };
    update(app, |s| s.announcement = Some(a));
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(seconds));
        while get(&app).is_some_and(|p| p.hovered.load(Ordering::Relaxed)) {
            std::thread::sleep(Duration::from_secs(1));
        }
        dismiss(&app, id);
    });
}

/// Takes a message away (its ✕, its button, or its time ran out). The page
/// asks again later while the pointer rests on it.
pub fn dismiss(app: &AppHandle, id: u64) {
    update(app, |s| {
        if s.announcement.as_ref().is_some_and(|a| a.id == id) {
            s.announcement = None;
        }
    });
}

/// Back to rest after a short message.
pub fn rest_after(app: &AppHandle, phase: &str, ms: u64) {
    let app = app.clone();
    let phase = phase.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        update(&app, |s| {
            if s.phase == phase {
                s.phase = "idle".into();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_names_and_sizes_match_expected() {
        assert_eq!(edge_name(Edge::Bottom), "bottom");
        assert_eq!(edge_name(Edge::BottomLeft), "bottom-left");
        assert_eq!(edge_name(Edge::BottomRight), "bottom-right");
        assert_eq!(edge_name(Edge::Left), "left");
        assert_eq!(edge_name(Edge::Right), "right");
        assert_eq!(edge_name(Edge::Top), "top");
        assert_eq!(edge_name(Edge::Hidden), "hidden");
    }

    #[test]
    fn a_dropped_pill_sticks_to_the_nearest_edge() {
        // Work area 1000×800 at (0, 0).
        let s = |x, y| snap(x, y, 0.0, 0.0, 1000.0, 800.0);
        assert_eq!(s(500.0, 790.0), (Edge::Bottom, 0.5));
        assert_eq!(s(250.0, 5.0), (Edge::Top, 0.25));
        assert_eq!(s(3.0, 400.0), (Edge::Left, 0.5));
        assert_eq!(s(990.0, 200.0), (Edge::Right, 0.25));
        // Dropped in the middle of the screen: still on an edge, never left floating.
        let (edge, _) = s(500.0, 380.0);
        assert_eq!(edge, Edge::Top);
        // Outside the work area: clamped onto it.
        assert_eq!(s(-50.0, 900.0).1, 0.0);
    }

    #[test]
    fn the_pill_keeps_clear_of_the_corners() {
        assert_eq!(along(0, 1000, 0.5, 130), 500);
        assert_eq!(along(0, 1000, 0.0, 130), 130);
        assert_eq!(along(0, 1000, 1.0, 130), 870);
        assert_eq!(along(100, 200, 0.0, 130), 200);

        assert_eq!(size_for("bottom"), BOTTOM);
        assert_eq!(size_for("bottom-left"), BOTTOM);
        assert_eq!(size_for("bottom-right"), BOTTOM);
        assert_eq!(size_for("left"), SIDE);
        assert_eq!(size_for("right"), SIDE);
        assert_eq!(size_for("top"), BOTTOM);
    }
}
