//! Everything the audio path needs, split on the one line that matters:
//! `render` is what the callback executes and must never allocate, lock,
//! or block; `device` is the cpal boundary that does all three at setup.
//! Both play [`crate::spec`] data, which `script` bakes.

mod device;
mod fx;
mod render;

pub use device::{open, stream, Output, Snapshot, Stats, Stream};
pub use render::Player;

/// The callback's render step, for `benches/` only: a bench links the
/// crate from outside, so it sees only what is public.
#[doc(hidden)]
pub mod bench {
    use super::render::{fill as render_fill, Player};
    use crate::spec::{Event, Freqs, Instrument};

    /// Fills `out` as the audio callback does, returning the frames written.
    pub fn fill(out: &mut [f32], channels: usize, player: &mut Player) -> u32 {
        render_fill(out, channels, player).frames
    }

    /// A player fed from a ring, as the TUI's is.
    pub fn live(
        instruments: Vec<Instrument>,
        freqs: Freqs,
        events: rtrb::Consumer<Event>,
        sample_rate: f32,
    ) -> Player {
        Player::live(instruments, freqs, events, sample_rate)
    }
}
