//! The player both benches measure: a chord of sustaining voices from a Lua
//! fixture, played past every attack and decay so each callback after costs
//! the same. A file under a folder, not `benches/*.rs`, so Cargo does not
//! build it as a bench of its own.

use std::path::Path;

use synth::audio::{bench::fill, Player};
use synth::script;

pub(crate) const RATE: u32 = 48_000;
const RATE_HZ: f32 = 48_000.0;
// a common callback size
pub(crate) const FRAMES: usize = 512;
pub(crate) const CHANNELS: usize = 2;

pub(crate) fn player(voices: usize, filtered: bool) -> Player {
    let fixture = format!("tests/lua/bench/voices_{voices}.lua");
    let patch = script::load(Path::new(&fixture), RATE, None).expect("bench fixture loads");
    let filter = if filtered { patch.filter } else { None };
    let mut player = Player::new(patch.timeline, patch.freqs, RATE_HZ).with_filter(filter);

    // two seconds: past every note-on, attack, and decay
    let mut out = buffer();
    for _ in 0..2 * RATE as usize / FRAMES {
        fill(&mut out, CHANNELS, &mut player);
    }
    player
}

/// One callback's worth of interleaved frames.
pub(crate) fn buffer() -> Vec<f32> {
    vec![0.0; FRAMES * CHANNELS]
}
