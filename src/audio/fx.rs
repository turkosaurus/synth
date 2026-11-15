//! Signal processors: one sample in, one sample out, with no idea whether
//! the input is a voice, a sum of voices, or an analog input.
//!
//! Everything here builds with `core` alone, so it can move into the
//! `no_std` core from #33 unchanged. `mise run test:core` checks that.

mod approx;
mod svf;

pub(crate) use svf::Svf;

#[cfg(test)]
mod tests;
