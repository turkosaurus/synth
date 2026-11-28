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

/// Every MIDI pitch, the most notes that can sound at once.
pub(crate) const PITCHES: usize = 128;

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
    /// move the scope to its next source
    Scope,
}

pub(crate) struct Keyboard {
    keymap: Keymap,
    gating: Gating,
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
    /// when `stop` was last pressed, so a second press can be a double tap
    stopped_at: Option<Instant>,
    last_key: Option<char>,
}

impl Keyboard {
    /// Sized up front: a map holds at most one entry per note key in the
    /// keymap, or one per pitch up to [`PITCHES`], so playing never grows one.
    pub(crate) fn new(keymap: Keymap, gating: Gating) -> Self {
        let keys = keymap.notes.len();
        Self {
            octave: keymap.octave,
            keymap,
            gating,
            held: HashMap::with_capacity(keys),
            sounding: None,
            release_at: HashMap::with_capacity(PITCHES),
            pressed_at: HashMap::with_capacity(keys),
            stopped_at: None,
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

    /// Read `stroke`, appending any note events it causes to `out`.
    pub(crate) fn handle(&mut self, stroke: Stroke, out: &mut Vec<Event>) -> Outcome {
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
                if self.keymap.scope.contains(&stroke.key) {
                    return Outcome::Scope;
                }
                self.press(stroke.key, out)
            }
            // release only arrives when gating is Held;
            // in Fixed mode, `tick` ends the note
            Kind::Release => {
                if let Key::Char(ch) = stroke.key {
                    self.release(ch, out);
                }
            }
            Kind::Repeat => {}
        }
        Outcome::Continue
    }

    fn press(&mut self, key: Key, out: &mut Vec<Event>) {
        // control bindings are matched as keys,
        // so "backspace" works alongside "="
        if self.keymap.stop.contains(&key) {
            self.stop(Instant::now(), out);
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
        let Some(&base) = self.keymap.notes.get(&ch) else {
            return;
        };
        if self.repeated(ch, Instant::now()) {
            return;
        }
        let Some(note) = shift(base, self.octave) else {
            return;
        };

        self.last_key = Some(ch);
        self.held.insert(ch, note);
        self.sounding = Some(note);
        out.push(Event::NoteOn {
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
            // the keymap's `autorepeat` is how close counts
            Gating::Fixed => self
                .pressed_at
                .insert(ch, now)
                .is_some_and(|last| now - last < self.keymap.autorepeat),
        }
    }

    fn release(&mut self, ch: char, out: &mut Vec<Event>) {
        if let Some(note) = self.held.remove(&ch) {
            self.end(note, out);
        }
    }

    /// Call once per poll iteration. Ends every `Fixed` note whose time is up.
    pub(crate) fn tick(&mut self, out: &mut Vec<Event>) {
        let now = Instant::now();
        // found one at a time rather than collected, which would allocate
        while let Some(note) = self
            .release_at
            .iter()
            .find(|&(_, &at)| now >= at)
            .map(|(&note, _)| note)
        {
            self.end(note, out);
        }
    }

    /// Emergency silence, bound to whatever the keymap's `stop` list names.
    /// Releases every voice, not only the latest, so nothing is left held.
    pub(crate) fn panic(&mut self, out: &mut Vec<Event>) {
        self.held.clear();
        self.sounding = None;
        self.release_at.clear();
        self.pressed_at.clear();
        out.push(Event::AllNotesOff {
            instrument: INSTRUMENT,
        });
    }

    /// A press of `stop`: All Notes Off at once, and All Sound Off as well
    /// when it is a double tap. The first press never waits to learn
    /// whether a second follows.
    fn stop(&mut self, now: Instant, out: &mut Vec<Event>) {
        self.panic(out);
        if self
            .stopped_at
            .is_some_and(|last| now - last < self.keymap.doubletap)
        {
            out.push(Event::AllSoundOff {
                instrument: INSTRUMENT,
            });
        }
        self.stopped_at = Some(now);
    }

    fn end(&mut self, note: u8, out: &mut Vec<Event>) {
        // also reached by tick(), which sees no key release, so this is the
        // only place `held` drains one note at a time
        self.held.retain(|_, held| *held != note);
        self.release_at.remove(&note);
        if self.sounding == Some(note) {
            self.sounding = None;
        }
        out.push(Event::NoteOff {
            instrument: INSTRUMENT,
            note,
        });
    }
}

fn shift(note: u8, octave: i32) -> Option<u8> {
    let shifted = note as i32 + octave * 12;
    (0..128).contains(&shifted).then_some(shifted as u8)
}

#[cfg(test)]
mod tests;
