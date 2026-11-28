//! Level meter ballistics: instant attack, timed hold, linear fallback,
//! on a decibel scale. Pure arithmetic, so it is tested without a
//! terminal or a device, the way `table::commas` is.

use std::time::Duration;

/// Amplitude to a 0.0..1.0 scale position, logarithmically, with `floor_db`
/// (below 0) at the bottom. Amplitudes at or below the floor read as empty.
///
/// Meters on real gear are logarithmic because hearing is: -20 dBFS is a
/// healthy signal that a linear bar would draw one tenth full.
pub(crate) fn scale(amplitude: f32, floor_db: f32) -> f32 {
    if amplitude <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * amplitude.log10();
    ((db - floor_db) / -floor_db).clamp(0.0, 1.0)
}

/// Inverse of [`scale`], for labelling a bar in the units it is drawn in.
pub(crate) fn db(scaled: f32, floor_db: f32) -> f32 {
    floor_db + scaled.clamp(0.0, 1.0) * -floor_db
}

/// A scale position as dB, padded for a label.
///
/// An empty bar reads -∞ rather than the floor: everything from silence up
/// to the floor lands on 0.0, and printing the floor would claim the
/// quietest audible signal when there may be none at all.
pub(crate) fn db_label(scaled: f32, floor_db: f32) -> String {
    if scaled <= 0.0 {
        return format!("{:>5}", "-∞");
    }
    format!("{:>5.1}", db(scaled, floor_db))
}

/// Milliseconds to a 0.0..1.0 scale position, logarithmically, from
/// `floor_ms` to `ceil_ms` (both above 0). At or below the floor reads as
/// empty.
///
/// Latency stages span four decades, and on a linear bar the ones under a
/// millisecond would draw as nothing beside the output device.
pub(crate) fn ms_scale(ms: f32, floor_ms: f32, ceil_ms: f32) -> f32 {
    if ms <= floor_ms {
        return 0.0;
    }
    ((ms / floor_ms).log10() / (ceil_ms / floor_ms).log10()).clamp(0.0, 1.0)
}

/// The powers of ten from `floor_ms` to `ceil_ms`, both ends included
/// where they are one, for ticks and labels on [`ms_scale`].
pub(crate) fn decades(floor_ms: f32, ceil_ms: f32) -> Vec<f32> {
    // a hair of slack, so 0.01 written in Lua counts as 10^-2
    let first = (floor_ms.log10() - 1e-4).ceil() as i32;
    let last = (ceil_ms.log10() + 1e-4).floor() as i32;
    (first..=last).map(|power| 10f32.powi(power)).collect()
}

/// A decade as an axis label: as many decimals as it needs, and none above 1.
pub(crate) fn decade_label(ms: f32) -> String {
    let decimals = (-ms.log10().round()).max(0.0) as usize;
    format!("{ms:.decimals$}")
}

/// Frames of history kept. A wider terminal shows more of it; the oldest
/// fall off the left edge.
const HISTORY: usize = 240;

/// Bar heights a level is quantised to.
///
/// A fixed ceiling, so bars are absolute: a given height always means the
/// same level. Letting the chart scale to its own contents would draw a
/// quiet passage as tall as a loud one.
pub(crate) const STEPS: u64 = 100;

/// Per-frame level, newest first, with the frames that clipped marked.
///
/// Height says how loud the frame was and the flag says whether it
/// clipped, because clipping is an attribute of a loud frame rather than a
/// separate measurement. `Stats` counts clipped samples cumulatively, so
/// the rate is recovered here by diffing consecutive readings.
/// Newest-first matches `RenderDirection::RightToLeft`, which draws index 0
/// at the right edge and drops whatever does not fit from the far end.
#[derive(Debug, Default)]
pub(crate) struct Activity {
    seen: u64,
    history: std::collections::VecDeque<(u64, bool)>,
}

impl Activity {
    /// Fold in one frame: a scale position, and the cumulative clip count.
    pub(crate) fn update(&mut self, scaled: f32, total: u64) {
        // saturating, so a counter that somehow went backwards reads as a
        // clean frame rather than wrapping to a vast spike
        let clipped = total.saturating_sub(self.seen) > 0;
        self.seen = total;

        let level = (scaled.clamp(0.0, 1.0) * STEPS as f32).round() as u64;
        self.history.push_front((level, clipped));
        self.history.truncate(HISTORY);
    }

    /// Clipped samples since the stream started.
    pub(crate) fn clipped(&self) -> u64 {
        self.seen
    }

    /// Newest first, as (height, clipped) pairs. Deliberately not a
    /// ratatui type: the arithmetic here stays testable without a terminal.
    pub(crate) fn history(&self) -> impl Iterator<Item = (u64, bool)> + '_ {
        self.history.iter().copied()
    }
}

/// A bar that rises instantly and falls slowly, plus a marker that parks
/// at the recent maximum.
///
/// Both positions are scale fractions, not amplitudes: the ballistics run
/// in the decibel domain, which is how a PPM's decay is specified.
#[derive(Debug, PartialEq)]
pub(crate) struct Meter {
    level: f32,
    hold: f32,
    /// time left before the marker begins to fall
    hold_left: Duration,
    /// how long a new maximum holds before the marker starts to fall
    hold_time: Duration,
    /// how long a fall across the whole scale takes, once falling
    fall_time: Duration,
}

impl Meter {
    /// An empty meter. `fall` must be above zero: the fall rate divides by
    /// it, and `config.lua` checks it.
    pub(crate) fn new(hold: Duration, fall: Duration) -> Self {
        Self {
            level: 0.0,
            hold: 0.0,
            hold_left: Duration::ZERO,
            hold_time: hold,
            fall_time: fall,
        }
    }

    /// Fold in one reading, `dt` after the last.
    pub(crate) fn update(&mut self, scaled: f32, dt: Duration) {
        let scaled = scaled.clamp(0.0, 1.0);
        let fall = dt.as_secs_f32() / self.fall_time.as_secs_f32();

        // attack is instant: a peak drawn late is a peak not drawn
        self.level = if scaled >= self.level {
            scaled
        } else {
            (self.level - fall).max(scaled)
        };

        if scaled >= self.hold {
            self.hold = scaled;
            self.hold_left = self.hold_time;
            return;
        }
        match self.hold_left.checked_sub(dt) {
            Some(left) => self.hold_left = left,
            None => {
                // only the part of dt past the window falls, so one long
                // frame cannot wipe the marker out in a single step
                let fall = (dt - self.hold_left).as_secs_f32() / self.fall_time.as_secs_f32();
                self.hold_left = Duration::ZERO;
                // the marker never falls below the bar it marks
                self.hold = (self.hold - fall).max(self.level);
            }
        }
    }

    pub(crate) fn level(&self) -> f32 {
        self.level
    }

    pub(crate) fn hold(&self) -> f32 {
        self.hold
    }
}

#[cfg(test)]
mod tests;
