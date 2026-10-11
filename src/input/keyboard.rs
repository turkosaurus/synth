use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::audio::Event;

/// An isomorphic grid resolved to MIDI notes, built from `lua/keyboard.lua`,
pub struct Keymap {
    pub notes: HashMap<char, u8>,
    pub octave_up: Vec<KeyCode>,
    pub octave_down: Vec<KeyCode>,
    pub stop: Vec<KeyCode>,
}

/// Parses a binding from the keymap. A single character is that key; anything
/// longer is a name, because keys like backspace have no character to write.
pub fn binding(name: &str) -> Option<KeyCode> {
    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(ch));
    }
    Some(match name.to_ascii_lowercase().as_str() {
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Char(' '),
        "esc" | "escape" => KeyCode::Esc,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        _ => return None,
    })
}

/// Default note duration when input device cannot (or will not) report key release.
pub const FIXED_NOTE: Duration = Duration::from_millis(200);

/// A key has no depth to time, so there is no velocity to measure. MIDI
/// hardware supplies a real one.
const VELOCITY: u8 = 100;

/// The keyboard plays the patch's first instrument.
const INSTRUMENT: usize = 0;

/// How a note ends. Only the keyboard needs this: a terminal may not report
/// key release, so in `Fixed` mode the poll loop schedules the note-off
/// itself. MIDI has real note-off and never comes through here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gating {
    Held,
    Fixed(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Continue,
    Quit,
}

pub struct Keyboard {
    keymap: Keymap,
    gating: Gating,
    tx: rtrb::Producer<Event>,
    octave: i32,
    /// The note each held key started. Recomputing it on release would use
    /// whatever octave is current by then, which orphans the note if the
    /// octave moved while the key was down.
    held: HashMap<char, u8>,
    sounding: Option<u8>,
    release_at: Option<Instant>,
    last_key: Option<char>,
}

impl Keyboard {
    pub fn new(keymap: Keymap, gating: Gating, tx: rtrb::Producer<Event>) -> Self {
        Self {
            keymap,
            gating,
            tx,
            octave: 0,
            held: HashMap::new(),
            sounding: None,
            release_at: None,
            last_key: None,
        }
    }

    pub fn octave(&self) -> i32 {
        self.octave
    }

    pub fn sounding(&self) -> Option<u8> {
        self.sounding
    }

    pub fn last_key(&self) -> Option<char> {
        self.last_key
    }

    pub fn handle(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // quit is ctrl-c/d only, hardcoded to survive an invalid keymap.
        // sound stop is configurable via the keymap's `stop` list.
        if matches!(key.code, KeyCode::Char('c' | 'd')) && ctrl {
            return Outcome::Quit;
        }

        match key.kind {
            KeyEventKind::Press => self.press(key.code),
            // release only arrives when gating is Held;
            // in Fixed mode, `tick` ends the note
            KeyEventKind::Release => {
                if let KeyCode::Char(ch) = key.code {
                    self.release(ch);
                }
            }
            KeyEventKind::Repeat => {}
        }
        Outcome::Continue
    }

    fn press(&mut self, code: KeyCode) {
        // control bindings are matched as key codes,
        // so "backspace" works alongside "="
        if self.keymap.stop.contains(&code) {
            self.panic();
            return;
        }
        if self.keymap.octave_up.contains(&code) {
            self.octave = (self.octave + 1).min(4);
            return;
        }
        if self.keymap.octave_down.contains(&code) {
            self.octave = (self.octave - 1).max(-4);
            return;
        }

        let KeyCode::Char(ch) = code else {
            return;
        };
        let Some(&base) = self.keymap.notes.get(&ch) else {
            return;
        };
        let Some(note) = shift(base, self.octave) else {
            return;
        };

        self.last_key = Some(ch);
        self.held.insert(ch, note);
        self.sounding = Some(note);
        self.send(Event::NoteOn {
            instrument: INSTRUMENT,
            note,
            velocity: VELOCITY,
        });

        self.release_at = match self.gating {
            Gating::Held => None,
            Gating::Fixed(length) => Some(Instant::now() + length),
        };
    }

    fn release(&mut self, ch: char) {
        if let Some(note) = self.held.remove(&ch) {
            self.end(note);
        }
    }

    /// Call once per poll iteration. Ends a `Fixed` note whose time is up.
    pub fn tick(&mut self) {
        if self.release_at.is_some_and(|at| Instant::now() >= at) {
            if let Some(note) = self.sounding {
                self.end(note);
            }
        }
    }

    /// Emergency silence, bound to whatever the keymap's `stop` list names.
    pub fn panic(&mut self) {
        if let Some(note) = self.sounding {
            self.end(note);
        }
    }

    fn end(&mut self, note: u8) {
        // also reached by tick() and panic(), neither of which sees a key
        // release, so this is the only place `held` is guaranteed to drain
        self.held.retain(|_, held| *held != note);
        if self.sounding == Some(note) {
            self.sounding = None;
            self.release_at = None;
        }
        self.send(Event::NoteOff {
            instrument: INSTRUMENT,
            note,
        });
    }

    // the queue is bounded; a full queue means the audio thread has stalled,
    // and dropping an event is better than blocking the UI
    fn send(&mut self, event: Event) {
        let _ = self.tx.push(event);
    }
}

fn shift(note: u8, octave: i32) -> Option<u8> {
    let shifted = note as i32 + octave * 12;
    (0..128).contains(&shifted).then_some(shifted as u8)
}
