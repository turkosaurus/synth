# benches

What one audio callback costs, measured two ways, to catch big regressions in the render path.

| bench | measures | use it for |
| --- | --- | --- |
| [`instructions.rs`](./instructions.rs) | instructions, counted by Callgrind under Valgrind (Linux) | regressions: counts repeat to about 0.005% on one CPU model, and differ by up to about 0.3% between models |
| [`render.rs`](./render.rs) | time, by criterion | real-world scale: time drifts 20 to 40% between runs on a laptop, with no code change |

Both play [`support/sustain.rs`](./support/sustain.rs): a chord of sustaining voices from [`tests/lua/bench/`](../tests/lua/bench), 512 stereo frames per callback at 48 kHz, which has a 10.7 ms deadline.

## running

- `mise run bench` runs both and saves `results/YYYYMMDD-HHMMSS.toml`; without Valgrind it saves time only
- `mise run bench:compare` compares the newest result with earlier ones
- [`bench.yml`](../.github/workflows/bench.yml) runs both daily on main, committing the result, and on PRs ready for review, commenting the comparison

## threshold

Instructions more than 5% above the earlier result are a regression: `bench:compare` marks the case and exits 2, which fails `bench.yml`. `BENCH_REGRESSION` sets another percentage. Time is never a regression on its own.

## adding a case

A change to the render path adds or updates a case that runs the code it changes, in both benches, so the change is measured before and after. A case that runs nothing new adds only time to every run.
