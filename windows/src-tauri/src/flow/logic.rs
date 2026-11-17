// The key logic of Flow dictation, with nothing from the OS in it. Port of
// FlowKeyLogic.swift.
//
// - Hold the key: Coucou listens. Release it: the words are pasted where you type.
// - Two quick taps: hands-free, until the next tap. With `one_tap` (a Flow
//   setting), one short press is enough.
// - Esc, or another key pressed while the key is held (a shortcut such as
//   Ctrl+C), cancels.
//
// Listening starts on the first press, so a word said right away is not lost; a
// press too short to be a hold waits a moment for the second tap (without
// `one_tap`), and is dropped if none comes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Idle,
    Holding { since: f64 },
    TapWait { until: f64 },
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// Open the microphone and show the pill.
    Start,
    /// Hands-free: keep listening after the key is released.
    Lock,
    /// Stop listening and paste what was said.
    Finish,
    /// Stop listening and paste nothing.
    Cancel,
    /// The last listen was dropped (too short) and a new one begins.
    Restart,
    /// A press after a lost release: the listen so far is pasted, and a new one begins.
    Renew,
}

/// A press shorter than this is a tap.
pub const HOLD_THRESHOLD: f64 = 0.30;
/// The second tap of a double tap comes within this, counted from the first release
/// (Windows' double-click time is 0.5 s from press to press).
pub const DOUBLE_TAP_WINDOW: f64 = 0.50;
/// Autorepeat sends a Down at least every 0.4 s (Windows' slowest rate) and its
/// first one within 1 s. A longer gap means the release was lost.
pub const STUCK_GAP: f64 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyLogic {
    phase: Phase,
    pub one_tap: bool,
    /// The key is down, as far as its events say.
    held: bool,
    /// When its last Down came.
    last_down: f64,
}

impl Default for KeyLogic {
    fn default() -> Self {
        Self { phase: Phase::Idle, one_tap: false, held: false, last_down: 0.0 }
    }
}

impl KeyLogic {
    #[cfg(test)]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_active(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub fn key_down(&mut self, t: f64) -> Action {
        // Down again while held: autorepeat, unless the gap says the release was lost.
        let repeat = self.held && t - self.last_down <= STUCK_GAP;
        self.held = true;
        self.last_down = t;
        match self.phase {
            Phase::Idle if repeat => Action::None, // still the key that just finished a listen
            Phase::Idle => {
                self.phase = Phase::Holding { since: t };
                Action::Start
            }
            Phase::Holding { .. } if repeat => Action::None, // key repeat
            Phase::Holding { .. } => {
                self.phase = Phase::Holding { since: t };
                Action::Renew
            }
            Phase::TapWait { until } => {
                if t <= until {
                    self.phase = Phase::Locked;
                    Action::Lock
                } else {
                    // The wait ran out unnoticed: start afresh.
                    self.phase = Phase::Holding { since: t };
                    Action::Restart
                }
            }
            // Held past the press that locked it: the repeats are not a second tap.
            Phase::Locked if repeat => Action::None,
            Phase::Locked => {
                self.phase = Phase::Idle;
                Action::Finish
            }
        }
    }

    pub fn key_up(&mut self, t: f64) -> Action {
        self.held = false;
        let Phase::Holding { since } = self.phase else { return Action::None };
        if t - since >= HOLD_THRESHOLD {
            self.phase = Phase::Idle;
            return Action::Finish;
        }
        if self.one_tap {
            self.phase = Phase::Locked;
            return Action::Lock;
        }
        self.phase = Phase::TapWait { until: t + DOUBLE_TAP_WINDOW };
        Action::None
    }

    /// Another key while the key is held, or between the two taps: it was a
    /// shortcut, not dictation.
    pub fn other_key(&mut self) -> Action {
        match self.phase {
            Phase::Holding { .. } | Phase::TapWait { .. } => {
                self.phase = Phase::Idle;
                Action::Cancel
            }
            Phase::Idle | Phase::Locked => Action::None,
        }
    }

    pub fn escape(&mut self) -> Action {
        if self.phase == Phase::Idle {
            return Action::None;
        }
        self.phase = Phase::Idle;
        Action::Cancel
    }

    /// Called when the double-tap window may have run out.
    pub fn tick(&mut self, t: f64) -> Action {
        match self.phase {
            Phase::TapWait { until } if t > until => {
                self.phase = Phase::Idle;
                Action::Cancel
            }
            _ => Action::None,
        }
    }

    /// Hands-free straight away (the pill's microphone button).
    pub fn lock(&mut self) {
        self.phase = Phase::Locked;
    }

    /// The listen ended on its own (no microphone, nothing heard…).
    pub fn reset(&mut self) {
        self.phase = Phase::Idle;
    }
}

/// The key held to dictate: a modifier on its own (holding it does nothing in
/// any app), Ctrl + Win, or a shortcut of the user's own (`Custom`, the keys in
/// `Prefs::key_combo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FlowKey {
    #[default]
    RightCtrl,
    RightAlt,
    RightShift,
    LeftCtrlSuper,
    LeftCtrl,
    LeftAlt,
    LeftShift,
    Custom,
}

/// The most keys a shortcut of one's own may hold.
pub const MAX_COMBO: usize = 4;

impl FlowKey {
    /// The keys to hold together, by their KeyboardEvent.code name
    /// ("ControlRight", "AltLeft", "KeyF"…): the platforms map those to their
    /// own codes. A custom shortcut that can't be used falls back to Right Ctrl.
    pub fn codes(self, combo: &[String]) -> Vec<String> {
        let fixed: &[&str] = match self {
            FlowKey::RightCtrl => &["ControlRight"],
            FlowKey::RightAlt => &["AltRight"],
            FlowKey::RightShift => &["ShiftRight"],
            FlowKey::LeftCtrlSuper => &["ControlLeft", "MetaLeft"],
            FlowKey::LeftCtrl => &["ControlLeft"],
            FlowKey::LeftAlt => &["AltLeft"],
            FlowKey::LeftShift => &["ShiftLeft"],
            FlowKey::Custom => {
                let mut out: Vec<String> = Vec::new();
                for code in combo {
                    if known_code(code) && !out.contains(code) {
                        out.push(code.clone());
                    }
                }
                if out.is_empty() || out.len() > MAX_COMBO {
                    return vec!["ControlRight".into()];
                }
                return out;
            }
        };
        fixed.iter().map(|c| c.to_string()).collect()
    }

    /// How the key is written in the log (the pages write their own).
    pub fn label(self, combo: &[String]) -> String {
        match self {
            FlowKey::RightCtrl => "Right Ctrl".into(),
            FlowKey::RightAlt => "Right Alt".into(),
            FlowKey::RightShift => "Right Shift".into(),
            FlowKey::LeftCtrl => "Left Ctrl".into(),
            FlowKey::LeftAlt => "Left Alt".into(),
            FlowKey::LeftShift => "Left Shift".into(),
            FlowKey::LeftCtrlSuper => if cfg!(windows) { "Ctrl + Win" } else { "Ctrl + Super" }.into(),
            FlowKey::Custom => self.codes(combo).join(" + "),
        }
    }
}

/// The key names a shortcut may use: modifiers, letters, digits, F1–F24 and
/// a few others every keyboard has in the same place.
pub fn known_code(code: &str) -> bool {
    is_modifier_code(code)
        || code.strip_prefix("Key").is_some_and(|c| c.len() == 1 && c.chars().all(|ch| ch.is_ascii_uppercase()))
        || code.strip_prefix("Digit").is_some_and(|c| c.len() == 1 && c.chars().all(|ch| ch.is_ascii_digit()))
        || code.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n))
        || matches!(code, "Space" | "Backquote" | "Insert" | "Pause" | "ScrollLock" | "CapsLock" | "ContextMenu")
}

pub fn is_modifier_code(code: &str) -> bool {
    matches!(
        code,
        "ControlLeft" | "ControlRight" | "AltLeft" | "AltRight" | "ShiftLeft" | "ShiftRight" | "MetaLeft" | "MetaRight"
    )
}

/// What a key going down or up does to a shortcut held to dictate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComboEdge {
    /// Not one of its keys.
    Outside,
    /// One of its keys, and nothing changes for dictation.
    Part,
    /// The last of its keys went down: hold to dictate starts.
    Down,
    /// Its keys are all held and one repeats.
    Repeat,
    /// One of its keys came up while all were held: hold to dictate ends.
    Up,
}

/// The keys of a shortcut, which are down: the same on every platform, the
/// platform only says which member a key is.
#[derive(Debug, Clone, Default)]
pub struct Combo {
    held: Vec<bool>,
}

impl Combo {
    pub fn new(len: usize) -> Self {
        Self { held: vec![false; len] }
    }

    /// Every key is down (Windows reads it to keep a shortcut's own letter).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn all_held(&self) -> bool {
        !self.held.is_empty() && self.held.iter().all(|&h| h)
    }

    /// Member `i` went down (`down`) or up.
    pub fn key(&mut self, i: usize, down: bool) -> ComboEdge {
        if i >= self.held.len() {
            return ComboEdge::Outside;
        }
        let was_all = self.held.iter().all(|&h| h);
        let was = std::mem::replace(&mut self.held[i], down);
        let now_all = self.held.iter().all(|&h| h);
        match (was_all, now_all) {
            (false, true) => ComboEdge::Down,
            (true, false) => ComboEdge::Up,
            (true, true) if down && was => ComboEdge::Repeat,
            _ => ComboEdge::Part,
        }
    }
}

/// Frequency bands for the pill's dots: low notes on the left, high on the right.
pub mod bands {
    pub const COUNT: usize = 9;
    pub const LOW_HZ: f64 = 90.0;
    pub const HIGH_HZ: f64 = 7000.0;

    /// The `count + 1` band edges in Hz, spaced evenly on a log scale.
    pub fn edges(count: usize) -> Vec<f64> {
        (0..=count).map(|i| LOW_HZ * (HIGH_HZ / LOW_HZ).powf(i as f64 / count as f64)).collect()
    }

    /// The FFT bins [from, to) of each band, for `fft_size` samples at `sample_rate`.
    /// Each band has at least one bin.
    pub fn bin_ranges(fft_size: usize, sample_rate: f64, count: usize) -> Vec<(usize, usize)> {
        let bin_hz = sample_rate / fft_size as f64;
        let max_bin = fft_size / 2;
        let e = edges(count);
        let mut out = Vec::with_capacity(count);
        let mut last = 1;
        for i in 0..count {
            let from = last.max((e[i] / bin_hz).floor() as usize);
            let to = max_bin.min((from + 1).max((e[i + 1] / bin_hz).ceil() as usize));
            out.push((from, to));
            last = to;
        }
        out
    }

    /// A band's power in dB → 0…1 for the dot's height: silence stays low, a
    /// normal voice fills it.
    pub fn level(db: f64) -> f64 {
        let (floor, ceiling) = (-78.0, -30.0);
        ((db - floor) / (ceiling - floor)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_of_each_choice() {
        assert_eq!(FlowKey::RightCtrl.codes(&[]), ["ControlRight"]);
        assert_eq!(FlowKey::LeftCtrlSuper.codes(&[]), ["ControlLeft", "MetaLeft"]);
        assert_eq!(FlowKey::LeftAlt.codes(&[]), ["AltLeft"]);
        let combo: Vec<String> = ["AltLeft", "ShiftLeft", "KeyF"].map(String::from).to_vec();
        assert_eq!(FlowKey::Custom.codes(&combo), combo);
        assert_eq!(FlowKey::Custom.label(&combo), "AltLeft + ShiftLeft + KeyF");
        // Nothing usable, unknown names, doubles, too many: Right Ctrl, or what is left.
        assert_eq!(FlowKey::Custom.codes(&[]), ["ControlRight"]);
        assert_eq!(FlowKey::Custom.codes(&["Hyper".into(), "KeyF".into(), "KeyF".into()]), ["KeyF"]);
        let five: Vec<String> = ["ControlLeft", "AltLeft", "ShiftLeft", "MetaLeft", "KeyF"].map(String::from).to_vec();
        assert_eq!(FlowKey::Custom.codes(&five), ["ControlRight"]);
        // The other keys' names, written to the prefs, are kept as they come.
        assert_eq!(serde_json::to_string(&FlowKey::LeftShift).unwrap(), "\"left_shift\"");
        assert_eq!(serde_json::from_str::<FlowKey>("\"custom\"").unwrap(), FlowKey::Custom);
    }

    #[test]
    fn a_shortcut_starts_when_its_last_key_goes_down_and_ends_with_the_first_up() {
        let mut c = Combo::new(3); // Alt, Shift, F
        assert_eq!(c.key(0, true), ComboEdge::Part);
        assert_eq!(c.key(1, true), ComboEdge::Part);
        assert_eq!(c.key(2, true), ComboEdge::Down);
        assert_eq!(c.key(2, true), ComboEdge::Repeat);
        assert!(c.all_held());
        assert_eq!(c.key(1, false), ComboEdge::Up);
        assert_eq!(c.key(2, false), ComboEdge::Part);
        assert_eq!(c.key(0, false), ComboEdge::Part);
        assert_eq!(c.key(7, true), ComboEdge::Outside);
        // Any order.
        assert_eq!(c.key(2, true), ComboEdge::Part);
        assert_eq!(c.key(0, true), ComboEdge::Part);
        assert_eq!(c.key(1, true), ComboEdge::Down);
        assert_eq!(c.key(0, false), ComboEdge::Up);
        assert!(!c.all_held());
        // One key on its own.
        let mut one = Combo::new(1);
        assert_eq!(one.key(0, true), ComboEdge::Down);
        assert_eq!(one.key(0, true), ComboEdge::Repeat);
        assert_eq!(one.key(0, false), ComboEdge::Up);
    }

    #[test]
    fn hold_then_release_pastes() {
        let mut k = KeyLogic::default();
        assert_eq!(k.key_down(0.0), Action::Start);
        assert_eq!(k.key_down(0.1), Action::None); // key repeat
        assert_eq!(k.key_up(1.2), Action::Finish);
        assert!(!k.is_active());
    }

    #[test]
    fn two_quick_taps_go_hands_free_and_a_third_pastes() {
        let mut k = KeyLogic::default();
        assert_eq!(k.key_down(0.0), Action::Start);
        assert_eq!(k.key_up(0.1), Action::None);
        assert_eq!(k.key_down(0.3), Action::Lock);
        assert_eq!(k.key_up(0.35), Action::None);
        assert_eq!(k.phase(), Phase::Locked);
        assert_eq!(k.key_down(5.0), Action::Finish);
    }

    #[test]
    fn a_lone_tap_is_dropped_once_the_window_runs_out() {
        let mut k = KeyLogic::default();
        k.key_down(0.0);
        k.key_up(0.1);
        assert_eq!(k.tick(0.3), Action::None);
        assert_eq!(k.tick(0.6), Action::None); // the window ends at 0.1 + 0.5
        assert_eq!(k.tick(0.65), Action::Cancel);
        assert!(!k.is_active());
        // A press after an unnoticed timeout starts afresh.
        let mut k = KeyLogic::default();
        k.key_down(0.0);
        k.key_up(0.1);
        assert_eq!(k.key_down(2.0), Action::Restart);
    }

    #[test]
    fn with_one_tap_a_short_press_goes_hands_free() {
        let mut k = KeyLogic { one_tap: true, ..Default::default() };
        assert_eq!(k.key_down(0.0), Action::Start);
        assert_eq!(k.key_up(0.1), Action::Lock);
        assert_eq!(k.phase(), Phase::Locked);
        assert_eq!(k.key_down(5.0), Action::Finish);
        assert_eq!(k.key_up(5.1), Action::None);
        assert!(!k.is_active());
        // A hold stays push-to-talk.
        assert_eq!(k.key_down(6.0), Action::Start);
        assert_eq!(k.key_up(7.0), Action::Finish);
        // A shortcut while the key is held still cancels.
        k.key_down(8.0);
        assert_eq!(k.other_key(), Action::Cancel);
        assert!(!k.is_active());
    }

    #[test]
    fn a_shortcut_or_esc_cancels() {
        let mut k = KeyLogic::default();
        k.key_down(0.0);
        assert_eq!(k.other_key(), Action::Cancel);
        let mut k = KeyLogic::default();
        k.lock();
        assert_eq!(k.other_key(), Action::None); // typing while hands-free is fine
        assert_eq!(k.escape(), Action::Cancel);
        assert_eq!(k.escape(), Action::None);
    }

    #[test]
    fn a_lost_release_does_not_hold_the_next_press() {
        let mut k = KeyLogic::default();
        assert_eq!(k.key_down(0.0), Action::Start);
        // The Up never came. The next press, seconds later, pastes the listen so far and starts again.
        assert_eq!(k.key_down(4.0), Action::Renew);
        assert_eq!(k.key_up(4.5), Action::Finish);
        assert!(!k.is_active());
    }

    #[test]
    fn autorepeat_while_held_is_not_a_new_press() {
        let mut k = KeyLogic::default();
        assert_eq!(k.key_down(0.0), Action::Start);
        // The first repeat after 0.5 s, then one every 0.4 s, the slowest rate.
        for i in 0..30 {
            assert_eq!(k.key_down(0.5 + i as f64 * 0.4), Action::None);
        }
        assert_eq!(k.key_up(13.0), Action::Finish);
        assert!(!k.is_active());
    }

    #[test]
    fn alt_tab_cancels_and_the_release_after_it_does_nothing() {
        let mut k = KeyLogic::default();
        assert_eq!(k.key_down(0.0), Action::Start); // Alt
        assert_eq!(k.other_key(), Action::Cancel); // Tab
        assert_eq!(k.key_up(0.2), Action::None); // Alt let go
        assert!(!k.is_active());
        // A new press after that starts a new dictation, not a Renew.
        assert_eq!(k.key_down(1.0), Action::Start);
    }

    #[test]
    fn a_second_tap_within_half_a_second_of_the_first_release_locks() {
        let mut k = KeyLogic::default();
        k.key_down(0.0);
        k.key_up(0.1);
        // 0.45 s after the first release: inside the window.
        assert_eq!(k.key_down(0.55), Action::Lock);
        k.key_up(0.6);
        assert_eq!(k.phase(), Phase::Locked);
        // Outside it: the first tap was not a double tap, and the press starts a listen.
        let mut k = KeyLogic::default();
        k.key_down(0.0);
        k.key_up(0.1);
        assert_eq!(k.key_down(0.7), Action::Restart);
    }

    #[test]
    fn a_held_second_tap_does_not_finish_at_its_first_repeat() {
        let mut k = KeyLogic::default();
        k.key_down(0.0);
        k.key_up(0.1);
        assert_eq!(k.key_down(0.25), Action::Lock);
        // The second press stays down: Windows repeats it after its delay.
        for t in [0.8, 1.2, 1.6, 2.0] {
            assert_eq!(k.key_down(t), Action::None);
        }
        assert_eq!(k.key_up(2.1), Action::None);
        assert!(k.is_active());
        // The next tap finishes; the key let go afterwards starts nothing.
        assert_eq!(k.key_down(3.0), Action::Finish);
        assert_eq!(k.key_up(3.1), Action::None);
        assert!(!k.is_active());
    }

    #[test]
    fn a_finishing_press_held_down_does_not_start_again() {
        let mut k = KeyLogic::default();
        k.key_down(0.0);
        k.key_up(0.1);
        k.key_down(0.25); // hands-free
        k.key_up(0.3);
        assert_eq!(k.key_down(3.0), Action::Finish);
        // Still held, repeating: no new dictation.
        assert_eq!(k.key_down(3.4), Action::None);
        assert_eq!(k.key_down(3.8), Action::None);
        assert!(!k.is_active());
        assert_eq!(k.key_up(4.0), Action::None);
        // A fresh press does start one.
        assert_eq!(k.key_down(5.0), Action::Start);
    }

    #[test]
    fn with_one_tap_the_next_tap_pastes_hands_free() {
        let mut k = KeyLogic { one_tap: true, ..Default::default() };
        assert_eq!(k.key_down(0.0), Action::Start);
        assert_eq!(k.key_up(0.1), Action::Lock);
        assert_eq!(k.key_down(4.0), Action::Finish);
        assert_eq!(k.key_up(4.1), Action::None);
        assert!(!k.is_active());
    }

    #[test]
    fn bands_cover_the_voice_from_low_to_high() {
        let e = bands::edges(9);
        assert_eq!(e.len(), 10);
        assert!((e[0] - 90.0).abs() < 1e-9 && (e[9] - 7000.0).abs() < 1e-6);
        let r = bands::bin_ranges(512, 16000.0, 9);
        assert_eq!(r.len(), 9);
        for (i, (from, to)) in r.iter().enumerate() {
            assert!(to > from, "band {i} is empty");
            if i > 0 {
                assert_eq!(*from, r[i - 1].1);
            }
        }
        assert_eq!(bands::level(-100.0), 0.0);
        assert_eq!(bands::level(0.0), 1.0);
    }
}
