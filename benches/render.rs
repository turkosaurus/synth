//! What one audio callback costs: `fill` for 1, 8, and 32 sustaining
//! voices, with and without the patch's filter. Throughput is in frames,
//! so criterion's elements per second is frames per second.

use std::hint::black_box;
use std::path::Path;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use synth::audio::{bench::fill, Player};
use synth::script;

const RATE: u32 = 48_000;
const RATE_HZ: f32 = 48_000.0;
// a common callback size
const FRAMES: usize = 512;
const CHANNELS: usize = 2;

fn player(voices: usize, filtered: bool) -> Player {
    let fixture = format!("tests/lua/bench/voices_{voices}.lua");
    let patch = script::load(Path::new(&fixture), RATE, None).expect("bench fixture loads");
    let filter = if filtered { patch.filter } else { None };
    let mut player = Player::new(patch.timeline, patch.freqs, RATE_HZ).with_filter(filter);

    // two seconds: past every note-on, attack, and decay, so each callback
    // measured after this costs the same
    let mut out = vec![0.0; FRAMES * CHANNELS];
    for _ in 0..2 * RATE as usize / FRAMES {
        fill(&mut out, CHANNELS, &mut player);
    }
    player
}

fn callback(c: &mut Criterion) {
    let mut group = c.benchmark_group("callback");
    group.throughput(Throughput::Elements(FRAMES as u64));
    for filtered in [false, true] {
        let name = if filtered { "filtered" } else { "dry" };
        for voices in [1, 8, 32] {
            let mut player = player(voices, filtered);
            let mut out = vec![0.0; FRAMES * CHANNELS];
            group.bench_function(BenchmarkId::new(name, voices), |b| {
                b.iter(|| fill(black_box(&mut out), CHANNELS, &mut player));
            });
        }
    }
    group.finish();
}

criterion_group!(benches, callback);
criterion_main!(benches);
