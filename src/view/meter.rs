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
/// separate measurement. `Stats` counts clips cumulatively, so the rate is
/// recovered here by diffing consecutive readings. Newest-first matches
/// `RenderDirection::RightToLeft`, which draws index 0 at the right edge
/// and drops whatever does not fit from the far end.
#[derive(Debug, Default)]
pub(crate) struct Activity {
    seen: u64,
    history: std::collections::VecDeque<(u64, bool)>,
}

impl Activity {
    /// Fold in one frame: a scale position, and the cumulative clip count.
    pub(crate) fn update(&mut self, scaled: f32, clips: u64) {
        // saturating, so a counter that somehow went backwards reads as a
        // clean frame rather than wrapping to a vast spike
        let clipped = clips.saturating_sub(self.seen) > 0;
        self.seen = clips;

        let level = (scaled.clamp(0.0, 1.0) * STEPS as f32).round() as u64;
        self.history.push_front((level, clipped));
        self.history.truncate(HISTORY);
    }

    /// Clips since the stream started.
    pub(crate) fn clips(&self) -> u64 {
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
mod tests {
    use super::{Activity, Meter, STEPS};
    use std::time::Duration;

    const FLOOR_DB: f32 = -60.0;
    const HOLD: Duration = Duration::from_millis(560);
    const FALL: Duration = Duration::from_millis(280);

    fn scale(amplitude: f32) -> f32 {
        super::scale(amplitude, FLOOR_DB)
    }

    fn db(scaled: f32) -> f32 {
        super::db(scaled, FLOOR_DB)
    }

    fn db_label(scaled: f32) -> String {
        super::db_label(scaled, FLOOR_DB)
    }

    fn meter() -> Meter {
        Meter::new(HOLD, FALL)
    }

    const FRAME: Duration = Duration::from_millis(16);

    #[test]
    fn full_scale_is_zero_db() {
        assert_eq!(scale(1.0), 1.0);
        assert_eq!(db(1.0), 0.0);
    }

    #[test]
    fn silence_sits_on_the_floor() {
        assert_eq!(scale(0.0), 0.0);
        assert_eq!(db(0.0), FLOOR_DB);
    }

    #[test]
    fn halving_amplitude_drops_about_six_db() {
        let six = db(scale(0.5));
        assert!((six + 6.02).abs() < 0.01, "0.5 amplitude read {six} dB");
    }

    #[test]
    fn scale_clamps_below_the_floor() {
        assert_eq!(scale(0.0001), 0.0);
    }

    #[test]
    fn a_rising_clip_count_marks_only_the_frames_it_rose_in() {
        let mut activity = Activity::default();
        for total in [0, 0, 3, 3, 10] {
            activity.update(0.0, total);
        }
        assert_eq!(activity.clips(), 10);
        // newest first: rose to 10, flat, rose to 3, flat, flat
        let marks: Vec<bool> = activity.history().map(|(_, clipped)| clipped).collect();
        assert_eq!(marks, vec![true, false, true, false, false]);
    }

    #[test]
    fn level_quantises_to_the_fixed_ceiling() {
        let mut activity = Activity::default();
        activity.update(1.0, 0);
        activity.update(0.5, 0);
        activity.update(0.0, 0);
        let heights: Vec<u64> = activity.history().map(|(level, _)| level).collect();
        assert_eq!(heights, vec![0, STEPS / 2, STEPS]);
    }

    #[test]
    fn activity_history_is_bounded() {
        let mut activity = Activity::default();
        for total in 0..(super::HISTORY as u64 * 2) {
            activity.update(0.5, total);
        }
        assert_eq!(activity.history().count(), super::HISTORY);
    }

    #[test]
    fn a_counter_going_backwards_reads_clean() {
        let mut activity = Activity::default();
        activity.update(0.0, 50);
        activity.update(0.0, 10);
        assert_eq!(activity.history().next(), Some((0, false)));
        assert_eq!(activity.clips(), 10);
    }

    #[test]
    fn an_empty_bar_labels_as_negative_infinity() {
        assert_eq!(db_label(0.0).trim(), "-∞");
        assert_eq!(db_label(-1.0).trim(), "-∞");
    }

    #[test]
    fn a_sounding_bar_labels_in_db() {
        assert_eq!(db_label(1.0).trim(), "0.0");
        assert_eq!(db_label(scale(0.5)).trim(), "-6.0");
    }

    #[test]
    fn labels_share_one_width() {
        for scaled in [0.0, 0.5, 1.0] {
            assert_eq!(db_label(scaled).chars().count(), 5, "{scaled}");
        }
    }

    #[test]
    fn attack_is_instant() {
        let mut meter = meter();
        meter.update(1.0, FRAME);
        assert_eq!(meter.level(), 1.0);
        assert_eq!(meter.hold(), 1.0);
    }

    // durations come from the constants, so retuning the feel of the meter
    // in config.lua cannot turn these into failures

    #[test]
    fn level_falls_across_the_fall_time() {
        let mut meter = meter();
        meter.update(1.0, FRAME);
        meter.update(0.0, FALL / 2);

        // half the fall time, so half the scale
        assert!((meter.level() - 0.5).abs() < 0.01, "{}", meter.level());
    }

    #[test]
    fn hold_waits_out_its_window() {
        let mut meter = meter();
        meter.update(1.0, FRAME);
        meter.update(0.0, HOLD / 2);
        assert_eq!(meter.hold(), 1.0);
    }

    #[test]
    fn hold_falls_once_its_window_expires() {
        let mut meter = meter();
        meter.update(1.0, FRAME);

        // a quarter of a fall past the window, so the marker has left the
        // top but is nowhere near the floor
        meter.update(0.0, HOLD + FALL / 4);
        let parked = meter.hold();
        assert!(parked < 1.0 && parked > meter.level(), "parked at {parked}");

        meter.update(0.0, FALL / 4);
        assert!(meter.hold() < parked, "marker stayed at {parked}");
    }

    #[test]
    fn hold_never_falls_below_the_bar() {
        let mut meter = meter();
        meter.update(1.0, FRAME);
        for _ in 0..200 {
            meter.update(0.4, Duration::from_millis(100));
        }
        assert_eq!(meter.level(), 0.4);
        assert!(meter.hold() >= meter.level());
    }

    #[test]
    fn a_reading_above_full_scale_is_clamped() {
        let mut meter = meter();
        meter.update(1.5, FRAME);
        assert_eq!(meter.level(), 1.0);
    }
}
