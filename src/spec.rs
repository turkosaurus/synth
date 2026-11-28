//! Plain data between `script` and `audio`: what a loaded patch is once
//! Lua is gone. `script` produces it and `audio` consumes it.
//!
//! Nothing here names `mlua`, `cpal`, or another module of this crate,
//! so a patch baked on one host can be played by an engine on another.
//! Runtime sharing, such as the `Arc` a sounding note holds, is added by
//! the consumer.

mod config;
mod diagnostic;
mod filter;
mod instrument;
mod keymap;
mod patch;

pub use config::{Budget, Config, LogConfig, StreamName, TuiConfig};
pub use diagnostic::{Diagnostic, DiagnosticKind, Severity};
pub use filter::{
    CutoffMod, Filter, FilterReport, Kind, Unit, MAX_CUTOFF, MIN_CUTOFF, MIN_DAMPING,
};
pub use instrument::{
    Event, Freqs, FreqsError, Instrument, InstrumentError, Lfo, Segment, Timeline, Wavetable,
};
pub(crate) use keymap::{Key, Keymap};
pub(crate) use patch::Node;
pub use patch::{Patch, Report};
