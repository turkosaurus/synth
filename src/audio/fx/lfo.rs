//! A low-frequency oscillator: a baked cycle read at a rate, -1..1 out.
//!
//! The cycle is a Lua waveform, baked the way an oscillator's is, so an
//! LFO has whatever shape a patch can write. One LFO is shared by every
//! voice it drives.

// nothing reads an LFO until a destination is wired to one, which is the
// next step of #49
#![cfg_attr(not(test), allow(dead_code))]

pub(crate) struct Lfo {
    /// cycles per sample: rate over sample rate
    step: f32,
    /// 0..1 through the current cycle
    phase: f32,
}

impl Lfo {
    /// `rate` in Hz. Below half the sample rate a cycle spans at least two
    /// samples; a rate that is not finite holds the LFO at its start.
    pub(crate) fn new(rate: f32, sample_rate: f32) -> Self {
        let step = rate / sample_rate;
        Self {
            step: if step.is_finite() {
                step.clamp(0.0, 0.5)
            } else {
                0.0
            },
            phase: 0.0,
        }
    }

    /// The next sample of `cycle`, read between the two entries either side.
    pub(crate) fn next(&mut self, cycle: &[f32]) -> f32 {
        let out = read(cycle, self.phase);
        self.phase += self.step;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }

    pub(crate) fn reset(&mut self) {
        self.phase = 0.0;
    }
}

fn read(cycle: &[f32], phase: f32) -> f32 {
    let n = cycle.len();
    if n == 0 {
        return 0.0;
    }
    let pos = phase * n as f32;
    let i = pos as usize;
    let along = pos - i as f32;
    let a = cycle[i % n];
    let b = cycle[(i + 1) % n];
    a + (b - a) * along
}
