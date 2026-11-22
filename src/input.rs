//! Key presses to note events. A layout is Lua's, checked into a `Keymap`
//! by `script`; this module reads keys against it and hands each `Event`
//! back to the session, which pushes it into the ring the audio callback
//! reads.

pub(crate) mod keyboard;

pub use keyboard::Gating;
