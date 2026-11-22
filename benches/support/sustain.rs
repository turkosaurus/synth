//! The players both benches measure, from a Lua fixture's chord:
//! - `player`: the chord as a sequence, sustaining, played past every
//!   attack and decay so each callback after costs the same
//! - `live`: the chord's instrument fed from a ring, as the TUI's is, with
//!   one note released and one started before every callback
//!
//! A file under a folder, not `benches/*.rs`, so Cargo does not build it as
//! a bench of its own.

use std::path::Path;

use synth::audio::bench::{fill, live as live_player};
use synth::audio::{Feed, Player};
use synth::script;
use synth::spec::{Event, Patch};

pub(crate) const RATE: u32 = 48_000;
const RATE_HZ: f32 = 48_000.0;
// a common callback size
pub(crate) const FRAMES: usize = 512;
pub(crate) const CHANNELS: usize = 2;
// two seconds: past every note-on, attack, and decay
const WARM_UP: usize = 2 * RATE as usize / FRAMES;

fn fixture(voices: usize) -> Patch {
    let fixture = format!("tests/lua/bench/voices_{voices}.lua");
    script::load(Path::new(&fixture), RATE, None).expect("bench fixture loads")
}

pub(crate) fn player(voices: usize, filtered: bool) -> Player {
    let patch = fixture(voices);
    let filter = if filtered { patch.filter } else { None };
    let mut player =
        Player::new(Feed::Timeline(patch.timeline), patch.freqs, RATE_HZ).with_filter(filter);

    let mut out = buffer();
    for _ in 0..WARM_UP {
        fill(&mut out, CHANNELS, &mut player);
    }
    player
}

/// A live player holding `voices` notes, and the keys that feed it. Warmed
/// up with a step before every callback, so every voice is busy and each
/// note started steals one.
pub(crate) fn live(voices: usize) -> (Player, Keys) {
    let patch = fixture(voices);
    let (feed, events) = rtrb::RingBuffer::new(QUEUE);
    let mut player = live_player(patch.timeline.instruments, patch.freqs, events, RATE_HZ);
    let mut keys = Keys {
        feed,
        voices,
        next: 0,
    };
    for i in 0..voices {
        keys.press(note(i));
    }

    let mut out = buffer();
    for _ in 0..WARM_UP {
        keys.step();
        fill(&mut out, CHANNELS, &mut player);
    }
    (player, keys)
}

// the default `queue` in config.lua; the next callback reads every step's
// events, so few are ever waiting
const QUEUE: usize = 256;
// notes the keys move through: wider than the most voices, so the notes
// held are always distinct
const SPAN: usize = 48;
const LOWEST: u8 = 37;

fn note(i: usize) -> u8 {
    LOWEST + (i % SPAN) as u8
}

/// Holds `voices` neighbouring notes and moves up one note a step.
pub(crate) struct Keys {
    feed: rtrb::Producer<Event>,
    voices: usize,
    next: usize,
}

impl Keys {
    /// Releases the lowest held note and starts the one above the highest.
    pub(crate) fn step(&mut self) {
        self.push(Event::NoteOff {
            instrument: 0,
            note: note(self.next),
        });
        self.press(note(self.next + self.voices));
        self.next = (self.next + 1) % SPAN;
    }

    fn press(&mut self, note: u8) {
        self.push(Event::NoteOn {
            instrument: 0,
            note,
            velocity: 100,
        });
    }

    fn push(&mut self, event: Event) {
        self.feed.push(event).expect("the bench ring has room");
    }
}

/// One callback's worth of interleaved frames.
pub(crate) fn buffer() -> Vec<f32> {
    vec![0.0; FRAMES * CHANNELS]
}
