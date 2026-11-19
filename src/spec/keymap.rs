use std::collections::HashMap;
use std::time::Duration;

/// A key a control binding can name. Only the keys a layout can spell, so
/// a layout is described without a terminal library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Char(char),
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

/// Characters mapped to MIDI notes, built from `lua/keyboard.lua`.
pub(crate) struct Keymap {
    pub notes: HashMap<char, u8>,
    /// The layout's name, if the patch gave one.
    pub name: Option<String>,
    pub octave_up: Vec<Key>,
    pub octave_down: Vec<Key>,
    pub stop: Vec<Key>,
    pub help: Vec<Key>,
    /// How long a note lasts in `Fixed` gating, from the keyboard's `gate`.
    pub gate: Duration,
    /// In `Fixed` gating, a press of a key this soon after its last press
    /// is taken as autorepeat and ignored, from the keyboard's `repeat_window`.
    pub repeat: Duration,
    /// Every note's velocity: a key has no depth to time, so there is none
    /// to measure. MIDI hardware supplies a real one.
    pub velocity: u8,
    /// How far, in octaves, the octave keys may move from the start.
    pub octave_range: i32,
}
