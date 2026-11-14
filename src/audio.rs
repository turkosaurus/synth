//! Everything the audio path needs, split on the one line that matters:
//! `render` is what the callback executes and must never allocate, lock,
//! or block; `device` is the cpal boundary that does all three at setup.
//! `instrument` is baked, immutable data that both sides read.

mod device;
mod render;
mod instrument;

pub use device::{open, stream, Output, Snapshot, Stats};
pub use render::{Player, Tally};
pub use instrument::{Event, Freqs, Timeline, Instrument};
