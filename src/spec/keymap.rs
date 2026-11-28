use std::collections::HashMap;
use std::time::Duration;

/// A key a control binding can name. Only the keys a layout can spell, so
/// a layout is described without a terminal library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Char(char),
    /// a character pressed with ctrl
    Ctrl(char),
    Backspace,
    Delete,
    Enter,
    Tab,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Characters mapped to MIDI notes, built from `lua/input/keyboard.lua`.
#[derive(Clone)]
pub(crate) struct Keymap {
    pub notes: HashMap<char, u8>,
    /// The layout's name, if the patch gave one.
    pub name: Option<String>,
    pub octave_up: Vec<Key>,
    pub octave_down: Vec<Key>,
    pub stop: Vec<Key>,
    /// Moves the scope to its next source.
    pub scope: Vec<Key>,
    pub help: Vec<Key>,
    /// Asks before quitting.
    pub quit: Vec<Key>,
    /// Quits without asking.
    pub quit_now: Vec<Key>,
    /// How long a note lasts in `Fixed` gating, from the keyboard's `gate`.
    pub gate: Duration,
    /// In `Fixed` gating, a press of a key this soon after its last press
    /// is taken as autorepeat and ignored, from the keyboard's
    /// `autorepeat_threshold`.
    pub autorepeat: Duration,
    /// A second press of a key this soon after the first is a double tap,
    /// from the keyboard's `doubletap_threshold`. `stop` uses it.
    pub doubletap: Duration,
    /// Every note's velocity: a key has no depth to time, so there is none
    /// to measure. MIDI hardware supplies a real one.
    pub velocity: u8,
    /// The octave shift the keyboard starts at; 0 plays `notes` as written.
    pub octave: i32,
    /// How many octaves the octave keys may move below the start.
    pub octave_low: i32,
    /// How many octaves the octave keys may move above the start.
    pub octave_high: i32,
}
