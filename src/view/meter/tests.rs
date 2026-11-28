use super::{decade_label, decades, ms_scale, Activity, Meter, STEPS};
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
fn each_decade_takes_an_equal_share_of_the_ms_scale() {
    let at = |ms| ms_scale(ms, 0.01, 100.0);
    assert_eq!(at(0.01), 0.0);
    assert!((at(0.1) - 0.25).abs() < 1e-6);
    assert!((at(1.0) - 0.5).abs() < 1e-6);
    assert!((at(10.0) - 0.75).abs() < 1e-6);
    assert_eq!(at(100.0), 1.0);
}

#[test]
fn decades_span_the_scale() {
    assert_eq!(decades(0.01, 100.0), [0.01, 0.1, 1.0, 10.0, 100.0]);
    assert_eq!(decades(0.05, 30.0), [0.1, 1.0, 10.0]);
    assert_eq!(decades(2.0, 3.0), Vec::<f32>::new());
}

#[test]
fn decades_label_with_the_decimals_they_need() {
    let labels: Vec<String> = decades(0.001, 1000.0)
        .into_iter()
        .map(decade_label)
        .collect();
    assert_eq!(labels, ["0.001", "0.01", "0.1", "1", "10", "100", "1000"]);
}

#[test]
fn the_ms_scale_clamps_at_both_ends() {
    assert_eq!(ms_scale(0.0, 0.01, 100.0), 0.0);
    assert_eq!(ms_scale(-1.0, 0.01, 100.0), 0.0);
    assert_eq!(ms_scale(1000.0, 0.01, 100.0), 1.0);
}

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
    assert_eq!(activity.clipped(), 10);
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
    assert_eq!(activity.clipped(), 10);
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
