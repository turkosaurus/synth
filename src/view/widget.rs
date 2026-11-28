//! Widgets that draw only: pure functions of data and settings into a
//! ratatui `Buffer`, so they can draw on an embedded display too. They use
//! ratatui, core, and alloc, never std; the clock, the terminal, and the
//! frame source stay with the runtime that feeds them.
#![warn(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

pub(crate) mod scope;
pub(crate) mod spectrum;
