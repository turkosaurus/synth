use std::collections::HashMap;
use std::time::Instant;

use crate::spec::{Event, Key, Keymap};

/// One key event, translated by the view from whatever its library reports,
/// so this module names no terminal library.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Stroke {
    pub(crate) key: Key,
    pub(crate) kind: Kind,
    pub(crate) ctrl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Press,
    Release,
    Repeat,
}

impl Stroke {
    /// Quit is ctrl-c/d only, hardcoded to survive an invalid keymap.
    fn quits(self) -> bool {
        self.ctrl && matches!(self.key, Key::Char('c' | 'd'))
    }
}

/// The keyboard plays the patch's first instrument.
const INSTRUMENT: usize = 0;

/// How a note ends. Only the keyboard needs this: a terminal may not report
/// key release, so in `Fixed` mode the poll loop schedules the note-off
/// itself, after the keymap's `gate`. MIDI has real note-off and never comes
/// through here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gating {
    Held,
    Fixed,
}

/// What the session should do about a key the keyboard has seen. Dispatch stays
/// here rather than in the view sniffing key codes, so the keymap remains the
/// one place a binding is decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Outcome {
    Continue,
    Quit,
    Help,
}

pub(crate) struct Keyboard {
    keymap: Keymap,
    gating: Gating,
    tx: rtrb::Producer<Event>,
    octave: i32,
    /// The note each held key started. Recomputing it on release would use
    /// whatever octave is current by then, which orphans the note if the
    /// octave moved while the key was down.
    held: HashMap<char, u8>,
    /// the latest note, for the view to report
    sounding: Option<u8>,
    /// when each `Fixed` note ends; one per note, so a chord releases whole
    release_at: HashMap<u8, Instant>,
    /// when each key last arrived as a press, repeats included, so a
    /// `Fixed` key held down stays one note however long the repeats run
    pressed_at: HashMap<char, Instant>,
    last_key: Option<char>,
}

impl Keyboard {
    pub(crate) fn new(keymap: Keymap, gating: Gating, tx: rtrb::Producer<Event>) -> Self {
        Self {
            octave: keymap.octave,
            keymap,
            gating,
            tx,
            held: HashMap::new(),
            sounding: None,
            release_at: HashMap::new(),
            pressed_at: HashMap::new(),
            last_key: None,
        }
    }

    pub(crate) fn gating(&self) -> Gating {
        self.gating
    }

    /// The lowest and highest shift the octave keys can reach.
    pub(crate) fn reach(&self) -> (i32, i32) {
        let start = self.keymap.octave;
        (
            start - self.keymap.octave_low,
            start + self.keymap.octave_high,
        )
    }

    /// The current octave shift, and whether it is the one the keyboard
    /// started at.
    pub(crate) fn octave(&self) -> (i32, bool) {
        (self.octave, self.octave == self.keymap.octave)
    }

    pub(crate) fn sounding(&self) -> Option<u8> {
        self.sounding
    }

    pub(crate) fn last_key(&self) -> Option<char> {
        self.last_key
    }

    /// The resolved layout, so a view can show what the bindings are rather
    /// than restate them in prose that drifts.
    pub(crate) fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Dispatch for a view that is showing something modal: only bindings
    /// that must work everywhere are honoured, and nothing sounds. Releases
    /// report `Continue`, so the press that opened an overlay is not undone
    /// by its own key coming back up.
    pub(crate) fn global(&self, stroke: Stroke) -> Outcome {
        if stroke.kind != Kind::Press {
            return Outcome::Continue;
        }
        if stroke.quits() {
            return Outcome::Quit;
        }
        if self.keymap.help.contains(&stroke.key) {
            return Outcome::Help;
        }
        Outcome::Continue
    }

    pub(crate) fn handle(&mut self, stroke: Stroke) -> Outcome {
        // sound stop is configurable via the keymap's `stop` list
        if stroke.quits() {
            return Outcome::Quit;
        }

        match stroke.kind {
            Kind::Press => {
                // tested before press(), so a layout that puts the help key
                // somewhere in the note grid cannot also sound it
                if self.keymap.help.contains(&stroke.key) {
                    return Outcome::Help;
                }
                self.press(stroke.key)
            }
            // release only arrives when gating is Held;
            // in Fixed mode, `tick` ends the note
            Kind::Release => {
                if let Key::Char(ch) = stroke.key {
                    self.release(ch);
                }
            }
            Kind::Repeat => {}
        }
        Outcome::Continue
    }

    fn press(&mut self, key: Key) {
        // control bindings are matched as keys,
        // so "backspace" works alongside "="
        if self.keymap.stop.contains(&key) {
            self.panic();
            return;
        }
        if self.keymap.octave_up.contains(&key) {
            self.octave = (self.octave + 1).min(self.reach().1);
            return;
        }
        if self.keymap.octave_down.contains(&key) {
            self.octave = (self.octave - 1).max(self.reach().0);
            return;
        }

        let Key::Char(ch) = key else {
            return;
        };
        if self.repeated(ch, Instant::now()) {
            return;
        }
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
            velocity: self.keymap.velocity,
        });

        // a retrigger replaces its pitch's deadline rather than adding one
        if let Gating::Fixed = self.gating {
            self.release_at
                .insert(note, Instant::now() + self.keymap.gate);
        }
    }

    /// Whether a press of `ch` is an autorepeat rather than a new keystroke.
    /// The terminal sends repeats as presses in both modes, so this decides
    /// from what the keyboard already knows.
    fn repeated(&mut self, ch: char, now: Instant) -> bool {
        match self.gating {
            // a key cannot go down twice without a release in between
            Gating::Held => self.held.contains_key(&ch),
            // no release ever arrives, so only timing tells them apart;
            // the keymap's `repeat` is how close counts
            Gating::Fixed => self
                .pressed_at
                .insert(ch, now)
                .is_some_and(|last| now - last < self.keymap.repeat),
        }
    }

    fn release(&mut self, ch: char) {
        if let Some(note) = self.held.remove(&ch) {
            self.end(note);
        }
    }

    /// Call once per poll iteration. Ends every `Fixed` note whose time is up.
    pub(crate) fn tick(&mut self) {
        let now = Instant::now();
        let due: Vec<u8> = self
            .release_at
            .iter()
            .filter(|&(_, &at)| now >= at)
            .map(|(&note, _)| note)
            .collect();
        for note in due {
            self.end(note);
        }
    }

    /// Emergency silence, bound to whatever the keymap's `stop` list names.
    /// Releases every voice, not only the latest, so nothing is left held.
    pub(crate) fn panic(&mut self) {
        self.held.clear();
        self.sounding = None;
        self.release_at.clear();
        self.pressed_at.clear();
        self.send(Event::AllNotesOff {
            instrument: INSTRUMENT,
        });
    }

    fn end(&mut self, note: u8) {
        // also reached by tick(), which sees no key release, so this is the
        // only place `held` drains one note at a time
        self.held.retain(|_, held| *held != note);
        self.release_at.remove(&note);
        if self.sounding == Some(note) {
            self.sounding = None;
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const REPEAT: Duration = Duration::from_millis(750);

    fn keyboard(gating: Gating, gate: Duration) -> (Keyboard, rtrb::Consumer<Event>) {
        let keymap = Keymap {
            notes: HashMap::from([('a', 60), ('s', 62), ('A', 72)]),
            name: None,
            octave_up: vec![],
            octave_down: vec![],
            stop: vec![Key::Backspace],
            help: vec![],
            gate,
            repeat: REPEAT,
            velocity: 100,
            octave: 0,
            octave_low: 4,
            octave_high: 4,
        };
        let (tx, rx) = rtrb::RingBuffer::new(16);
        (Keyboard::new(keymap, gating, tx), rx)
    }

    fn stroke(key: Key, kind: Kind) -> Stroke {
        Stroke {
            key,
            kind,
            ctrl: false,
        }
    }

    fn press(keyboard: &mut Keyboard, key: Key) {
        keyboard.handle(stroke(key, Kind::Press));
    }

    /// A held-gating keyboard starting at `octave`, with `low` and `high`
    /// octaves of room, and `-` and `=` as its octave keys.
    fn octaves(octave: i32, low: i32, high: i32) -> (Keyboard, rtrb::Consumer<Event>) {
        let (mut keyboard, rx) = keyboard(Gating::Held, Duration::from_millis(200));
        keyboard.keymap.octave = octave;
        keyboard.keymap.octave_low = low;
        keyboard.keymap.octave_high = high;
        keyboard.keymap.octave_down = vec![Key::Char('-')];
        keyboard.keymap.octave_up = vec![Key::Char('=')];
        keyboard.octave = octave;
        (keyboard, rx)
    }

    #[test]
    fn the_start_octave_shifts_every_note() {
        let (mut keyboard, mut rx) = octaves(-1, 4, 4);
        press(&mut keyboard, Key::Char('a'));
        assert_eq!(ons(&mut rx), vec![48], "60 an octave down");
        assert_eq!(keyboard.octave(), (-1, true));
    }

    #[test]
    fn the_octave_keys_stop_at_separate_limits() {
        let (mut keyboard, _rx) = octaves(-1, 3, 4);
        for _ in 0..10 {
            press(&mut keyboard, Key::Char('-'));
        }
        assert_eq!(keyboard.octave(), (-4, false), "three below the start");
        for _ in 0..10 {
            press(&mut keyboard, Key::Char('='));
        }
        assert_eq!(keyboard.octave(), (3, false), "four above the start");
        assert_eq!(keyboard.reach(), (-4, 3));
    }

    fn release(keyboard: &mut Keyboard, key: Key) {
        keyboard.handle(stroke(key, Kind::Release));
    }

    fn ons(rx: &mut rtrb::Consumer<Event>) -> Vec<u8> {
        drain(rx)
            .into_iter()
            .filter_map(|e| match e {
                Event::NoteOn { note, .. } => Some(note),
                _ => None,
            })
            .collect()
    }

    fn drain(rx: &mut rtrb::Consumer<Event>) -> Vec<Event> {
        std::iter::from_fn(|| rx.pop().ok()).collect()
    }

    #[test]
    fn fixed_gating_ends_every_note_in_a_chord() {
        let (mut keyboard, mut rx) = keyboard(Gating::Fixed, Duration::ZERO);
        press(&mut keyboard, Key::Char('a'));
        press(&mut keyboard, Key::Char('s'));
        drain(&mut rx);

        keyboard.tick();
        // HashMap order, so sort before comparing
        let mut offs: Vec<u8> = drain(&mut rx)
            .into_iter()
            .filter_map(|e| match e {
                Event::NoteOff { note, .. } => Some(note),
                _ => None,
            })
            .collect();
        offs.sort();
        assert_eq!(offs, vec![60, 62]);
        assert!(keyboard.held.is_empty());
    }

    #[test]
    fn fixed_gating_waits_for_the_deadline() {
        let (mut keyboard, mut rx) = keyboard(Gating::Fixed, Duration::from_secs(60));
        press(&mut keyboard, Key::Char('a'));
        drain(&mut rx);

        keyboard.tick();
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn panic_releases_everything_once() {
        let (mut keyboard, mut rx) = keyboard(Gating::Held, Duration::ZERO);
        press(&mut keyboard, Key::Char('a'));
        press(&mut keyboard, Key::Char('s'));
        drain(&mut rx);

        press(&mut keyboard, Key::Backspace);
        assert_eq!(
            drain(&mut rx),
            vec![Event::AllNotesOff {
                instrument: INSTRUMENT
            }]
        );
        assert!(keyboard.held.is_empty());
        assert_eq!(keyboard.sounding(), None);
    }

    #[test]
    fn held_gating_ignores_autorepeat() {
        let (mut keyboard, mut rx) = keyboard(Gating::Held, Duration::ZERO);
        for _ in 0..3 {
            press(&mut keyboard, Key::Char('a'));
        }
        assert_eq!(ons(&mut rx), vec![60]);
    }

    #[test]
    fn held_gating_retriggers_after_release() {
        let (mut keyboard, mut rx) = keyboard(Gating::Held, Duration::ZERO);
        press(&mut keyboard, Key::Char('a'));
        release(&mut keyboard, Key::Char('a'));
        press(&mut keyboard, Key::Char('a'));
        assert_eq!(ons(&mut rx), vec![60, 60]);
    }

    #[test]
    fn held_gating_plays_and_releases_shifted_keys() {
        let (mut keyboard, mut rx) = keyboard(Gating::Held, Duration::ZERO);
        press(&mut keyboard, Key::Char('A'));
        press(&mut keyboard, Key::Char('A'));
        release(&mut keyboard, Key::Char('A'));
        assert_eq!(
            drain(&mut rx),
            vec![
                Event::NoteOn {
                    instrument: INSTRUMENT,
                    note: 72,
                    velocity: 100,
                },
                Event::NoteOff {
                    instrument: INSTRUMENT,
                    note: 72,
                },
            ]
        );
    }

    #[test]
    fn fixed_gating_ignores_autorepeat() {
        let (mut keyboard, mut rx) = keyboard(Gating::Fixed, Duration::ZERO);
        press(&mut keyboard, Key::Char('a'));
        // the note ends before the first repeat, as with a short gate
        keyboard.tick();
        press(&mut keyboard, Key::Char('a'));
        assert_eq!(ons(&mut rx), vec![60]);
    }

    #[test]
    fn fixed_gating_window_follows_the_repeats() {
        let (mut keyboard, _rx) = keyboard(Gating::Fixed, Duration::ZERO);
        let start = Instant::now();
        let step = REPEAT / 2;
        assert!(!keyboard.repeated('a', start));
        // each repeat restarts the window, so a long hold stays one note
        assert!(keyboard.repeated('a', start + step));
        assert!(keyboard.repeated('a', start + step * 2));
        assert!(keyboard.repeated('a', start + step * 3));
        // a press after a full window of quiet is a new keystroke
        assert!(!keyboard.repeated('a', start + step * 3 + REPEAT));
        // keys are timed separately
        assert!(!keyboard.repeated('s', start + step));
    }

    #[test]
    fn fixed_gating_zero_window_retriggers_every_press() {
        let (mut keyboard, mut rx) = keyboard(Gating::Fixed, Duration::ZERO);
        keyboard.keymap.repeat = Duration::ZERO;
        press(&mut keyboard, Key::Char('a'));
        press(&mut keyboard, Key::Char('a'));
        assert_eq!(ons(&mut rx), vec![60, 60]);
    }
}
