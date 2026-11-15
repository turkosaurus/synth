//! A filter as plain data: what a patch asks for, and what `check`
//! reports about it. `audio::fx` turns a [`Filter`] into a running filter.

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
pub const MIN_CUTOFF: f32 = 10.0;

/// Highest cutoff as a share of the sample rate. `tan` diverges at 0.5.
pub const MAX_CUTOFF: f32 = 0.49;

/// What `check` shows about a patch's filter, in the units Lua used.
#[derive(Debug)]
pub struct FilterReport {
    /// the constructor's name, e.g. `low_shelf`
    pub kind: &'static str,
    /// Hz, as written; the audio side may clamp it, which `warnings` says
    pub cutoff: f64,
    /// 0 to 1; `None` on shelves, which have none
    pub resonance: Option<f64>,
    /// dB; `None` on kinds without gain
    pub gain: Option<f64>,
    pub warnings: Vec<String>,
}
