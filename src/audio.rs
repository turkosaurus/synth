//! Everything the audio path needs, split on the one line that matters:
//! `render` is what the callback executes and must never allocate, lock,
//! or block; `device` is the cpal boundary that does all three at setup.
//! Both play [`crate::spec`] data, which `script` bakes.

mod device;
mod render;

pub use device::{open, stream, Output, Snapshot, Stats, Stream};
pub use render::Player;
