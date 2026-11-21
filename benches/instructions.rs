//! Instructions for one audio callback, counted by Callgrind under Valgrind:
//! the cases `render.rs` times, but a count does not drift with the
//! machine's clock or load, so runs on different days, or on CI runners,
//! compare. Setup (loading the patch, the warm-up) is not counted.
//!
//! Valgrind runs on Linux; elsewhere this bench is an empty program, so
//! clippy can still build every target.

#[cfg(target_os = "linux")]
use std::hint::black_box;

#[cfg(target_os = "linux")]
use gungraun::prelude::*;
#[cfg(target_os = "linux")]
use synth::audio::{bench::fill, Player};

#[cfg(target_os = "linux")]
#[path = "support/sustain.rs"]
mod sustain;

#[cfg(target_os = "linux")]
use sustain::{buffer, player, CHANNELS};

#[cfg(target_os = "linux")]
fn dry(voices: usize) -> (Player, Vec<f32>) {
    (player(voices, false), buffer())
}

#[cfg(target_os = "linux")]
fn filtered(voices: usize) -> (Player, Vec<f32>) {
    (player(voices, true), buffer())
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::dry_1(args = (1), setup = dry)]
#[bench::dry_8(args = (8), setup = dry)]
#[bench::dry_32(args = (32), setup = dry)]
#[bench::filtered_1(args = (1), setup = filtered)]
#[bench::filtered_8(args = (8), setup = filtered)]
#[bench::filtered_32(args = (32), setup = filtered)]
fn callback((mut player, mut out): (Player, Vec<f32>)) -> u32 {
    black_box(fill(black_box(&mut out), CHANNELS, &mut player))
}

#[cfg(target_os = "linux")]
library_benchmark_group!(name = render, benchmarks = [callback]);

#[cfg(target_os = "linux")]
main!(library_benchmark_groups = render);

#[cfg(not(target_os = "linux"))]
fn main() {}
