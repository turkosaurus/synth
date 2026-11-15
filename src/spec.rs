//! Plain data between `script` and `audio`: what a loaded patch is once
//! Lua is gone. `script` produces it and `audio` consumes it.
//!
//! Nothing here names `mlua`, `cpal`, or another module of this crate,
//! so a patch baked on one host can be played by an engine on another.
//! Runtime sharing, such as the `Arc` a sounding note holds, is added by
//! the consumer.

mod instrument;
mod keymap;
mod patch;

pub use instrument::{Event, Freqs, Instrument, Timeline};
pub(crate) use keymap::{Key, Keymap};
pub use patch::{Patch, Report};
