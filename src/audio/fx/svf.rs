//! State-variable filter in the TPT form from Andrew Simper's
//! "Linear Trapezoidal Integrated SVF" (Cytomic, 2013).
//!
//! One structure computes lowpass, bandpass, and highpass from shared
//! state, and every kind is a mix of those three and the input. It stays
//! stable for any positive `g`, so the cutoff may move on every sample,
//! and an approximate `tan` only shifts where the cutoff lands.

use core::f32::consts::PI;

use super::approx;

/// Which mix of the filter's outputs is heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Bell,
    LowShelf,
    HighShelf,
}

/// A filter as plain data: what Lua configured, before any state exists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filter {
    pub kind: Kind,
    /// Hz
    pub cutoff: f32,
    /// Simper's `k`, `1 / Q`: `SQRT_2` is flat (Butterworth), and
    /// smaller values resonate, down to `MIN_DAMPING`.
    pub damping: f32,
    /// `10^(dB / 80)`, the square root of the shelf amplitude `A`.
    /// Carried as the root so `A = root * root` needs no `sqrt`.
    /// 1.0 for kinds without gain.
    pub root: f32,
}

/// Damping at full resonance: rings, but never self-oscillates.
pub const MIN_DAMPING: f32 = 0.05;

/// Lowest cutoff, which keeps `g` above zero.
const MIN_CUTOFF: f32 = 10.0;

/// Highest cutoff as a share of the sample rate. `tan` diverges at 0.5.
const MAX_CUTOFF: f32 = 0.49;

/// How long the cutoff takes to glide to a new value, roughly.
const SMOOTH_MS: f32 = 5.0;

/// Below this the cutoff counts as arrived and stops being recomputed.
const SETTLED_HZ: f32 = 1e-3;

/// Below this, state snaps to zero. Decaying state would otherwise
/// become denormal, which is very slow on x86.
const DENORMAL: f32 = 1e-20;

pub(crate) struct Svf {
    kind: Kind,
    sample_rate: f32,
    damping: f32,
    /// shelf amplitude `A`, and its square root
    amp: f32,
    root: f32,

    cutoff: f32,
    target: f32,
    /// smoother step per sample
    glide: f32,

    // coefficients, recomputed whenever the cutoff moves
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,

    // integrator state
    ic1: f32,
    ic2: f32,
}

impl Svf {
    pub(crate) fn new(filter: Filter, sample_rate: f32) -> Self {
        let mut svf = Self {
            kind: filter.kind,
            sample_rate,
            damping: filter.damping,
            amp: filter.root * filter.root,
            root: filter.root,
            cutoff: 0.0,
            target: 0.0,
            // a one-pole smoother: c = 1 / (1 + time constant in samples)
            glide: 1.0 / (1.0 + SMOOTH_MS / 1000.0 * sample_rate),
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        svf.set_cutoff(filter.cutoff);
        svf.cutoff = svf.target;
        svf.coefficients();
        svf
    }

    /// Glide toward a new cutoff in Hz, clamped to the valid range.
    pub(crate) fn set_cutoff(&mut self, hz: f32) {
        // max then min rather than clamp: at a sample rate so low that the
        // ceiling falls under MIN_CUTOFF, the ceiling wins instead of panicking
        self.target = hz.max(MIN_CUTOFF).min(MAX_CUTOFF * self.sample_rate);
    }

    /// The cutoff in Hz as it currently stands, partway through any glide.
    /// Only tests read it today; #49 modulation will.
    #[cfg(test)]
    pub(crate) fn cutoff(&self) -> f32 {
        self.cutoff
    }

    pub(crate) fn next(&mut self, x: f32) -> f32 {
        self.glide();

        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);

        let (lp, bp, k, a) = (v2, v1, self.k, self.amp);
        let hp = x - k * bp - lp;
        match self.kind {
            Kind::Lowpass => lp,
            Kind::Highpass => hp,
            Kind::Bandpass => k * bp,
            Kind::Notch => lp + hp,
            Kind::Bell => x + k * (a * a - 1.0) * bp,
            Kind::LowShelf => x + k * (a - 1.0) * bp + (a * a - 1.0) * lp,
            Kind::HighShelf => a * a * x + k * (1.0 - a) * a * bp + (1.0 - a * a) * lp,
        }
    }

    fn glide(&mut self) {
        let gap = self.target - self.cutoff;
        if gap == 0.0 {
            return;
        }
        if gap.abs() < SETTLED_HZ {
            self.cutoff = self.target;
        } else {
            self.cutoff += gap * self.glide;
        }
        self.coefficients();
    }

    fn coefficients(&mut self) {
        let g = approx::tan(PI * self.cutoff / self.sample_rate);
        let (g, k) = match self.kind {
            Kind::LowShelf => (g / self.root, self.damping),
            Kind::HighShelf => (g * self.root, self.damping),
            Kind::Bell => (g, self.damping / self.amp),
            _ => (g, self.damping),
        };
        self.k = k;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
}

fn flush(x: f32) -> f32 {
    if x.abs() < DENORMAL {
        0.0
    } else {
        x
    }
}
