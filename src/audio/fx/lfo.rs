//! A low-frequency oscillator: a fixed shape read at a rate, -1..1 out.

// nothing reads an LFO until a destination is wired to one, which is the
// next step of #49
#![cfg_attr(not(test), allow(dead_code))]

use super::approx;

/// Each starts where `wave.lua`'s waves do: at 0 and rising, except the
/// square, which starts high.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    Sine,
    Triangle,
    Saw,
    Square,
}

pub(crate) struct Lfo {
    shape: Shape,
    /// cycles per sample: rate over sample rate
    step: f32,
    /// 0..1 through the current cycle
    phase: f32,
}

impl Lfo {
    /// `rate` in Hz. Below half the sample rate a cycle spans at least two
    /// samples; a rate that is not finite holds the LFO at its start.
    pub(crate) fn new(shape: Shape, rate: f32, sample_rate: f32) -> Self {
        let step = rate / sample_rate;
        Self {
            shape,
            step: if step.is_finite() {
                step.clamp(0.0, 0.5)
            } else {
                0.0
            },
            phase: 0.0,
        }
    }

    pub(crate) fn next(&mut self) -> f32 {
        let out = at(self.shape, self.phase);
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

fn at(shape: Shape, phase: f32) -> f32 {
    match shape {
        Shape::Sine => approx::sin_turn(phase),
        Shape::Triangle => {
            if phase < 0.25 {
                4.0 * phase
            } else if phase < 0.75 {
                2.0 - 4.0 * phase
            } else {
                4.0 * phase - 4.0
            }
        }
        Shape::Saw => {
            if phase < 0.5 {
                2.0 * phase
            } else {
                2.0 * phase - 2.0
            }
        }
        Shape::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }
}
