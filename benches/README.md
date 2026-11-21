# benches

What one audio callback costs, measured two ways, to catch big regressions in the render path.

| bench | measures | use it for |
| --- | --- | --- |
| [`instructions.rs`](./instructions.rs) | instructions, counted by Callgrind under Valgrind (Linux) | regressions: counts repeat to about 0.005% on one CPU model, and differ by up to about 0.3% between models |
| [`render.rs`](./render.rs) | time, by criterion | real-world scale: time drifts 20 to 40% between runs on a laptop, with no code change |

Both play [`support/sustain.rs`](./support/sustain.rs), 512 stereo frames per callback at 48 kHz, which has a 10.7 ms deadline. Each case plays a chord of 1, 8, or 32 voices from [`tests/lua/bench/`](../tests/lua/bench):

| case | plays | measures |
| --- | --- | --- |
| `dry` | the chord as a sequence, sustaining | the per-sample loop, summing the voices, and the timeline check on every sample |
| `filtered` | the same, through the patch's filter | the same, plus the filter |
| `live` | the chord's instrument fed from a ring, as the TUI's is; before every callback one note is released and one started, and each note started steals a voice | the ring check on every sample, note-on, note-off, and voice stealing |

## running

- `mise run bench` runs both benches, saves `results/YYYYMMDD-HHMMSS.toml`, and compares it with earlier results; without Valgrind it saves time only
- `mise run bench:record` and `mise run bench:compare` are the two halves
- [`bench.yml`](../.github/workflows/bench.yml) runs both on each merge to main and daily, committing the result; on PRs ready for review, it benches the PR once and comments a comparison with main's last result

## threshold

Instructions more than 5% above the earlier result are a regression: `bench:compare` marks the case and exits 2, which fails `bench.yml`. `BENCH_REGRESSION` sets another percentage. Time is never a regression on its own.

Main's ruleset requires the `bench` check, so a PR that regresses does not merge. A PR that costs more on purpose merges by admin bypass, and the comment records what it cost.

## adding a case

A change to the render path adds or updates a case that runs the code it changes, in both benches, so the change is measured before and after. A case that runs nothing new adds only time to every run.
