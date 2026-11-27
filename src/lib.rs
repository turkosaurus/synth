//! Lua-defined, Rust-driven synthesizer and sequencer.
//!
//! A patch is a Lua file. [`script`] loads it, checks it, and bakes it into
//! [`spec`] data, which [`audio`] plays on the realtime path. Live, a
//! [`session`] wires the device, the patch, and the ring that carries
//! [`input`]'s note events to the audio callback, and a [`view`] draws it.
//!
//! `mlua` stays in `script`, `cpal` in `audio`'s device, `crossterm` in
//! `view` and [`terminal`], and `spec` names none of them.

pub mod audio;
pub mod input;
pub mod install;
pub mod log;
pub mod release;
pub mod script;
pub mod session;
pub mod spec;
pub mod terminal;
pub mod update;
pub mod view;
