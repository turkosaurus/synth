//! Time for one audio callback: `fill` for 1, 8, and 32 sustaining voices,
//! with and without the patch's filter. Throughput is in frames, so
//! criterion's elements per second is frames per second. Time drifts with
//! the machine's clock and load; `instructions.rs` counts the same work.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use synth::audio::bench::fill;

#[path = "support/sustain.rs"]
mod sustain;

use sustain::{buffer, player, CHANNELS, FRAMES};

fn callback(c: &mut Criterion) {
    let mut group = c.benchmark_group("callback");
    group.throughput(Throughput::Elements(FRAMES as u64));
    for filtered in [false, true] {
        let name = if filtered { "filtered" } else { "dry" };
        for voices in [1, 8, 32] {
            let mut player = player(voices, filtered);
            let mut out = buffer();
            group.bench_function(BenchmarkId::new(name, voices), |b| {
                b.iter(|| fill(black_box(&mut out), CHANNELS, &mut player));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, callback);
criterion_main!(benches);
