use core::f32::consts::PI;

use super::approx;

#[test]
fn tan_tracks_std() {
    let top = 0.49 * PI;
    for i in 0..=1000 {
        let x = top * i as f32 / 1000.0;
        let want = x.tan();
        let got = approx::tan(x);
        assert!(
            (got - want).abs() <= 1e-3 * want.max(1e-3),
            "tan({x}) = {got}, want {want}"
        );
    }
}

use core::f32::consts::SQRT_2;

use super::Svf;
use crate::spec::{Filter, Kind, MIN_DAMPING};

const FS: f32 = 48000.0;
const N: usize = 8192;

fn lowpass(cutoff: f32, damping: f32) -> Filter {
    Filter {
        kind: Kind::Lowpass,
        cutoff,
        damping,
        root: 1.0,
    }
}

fn at(kind: Kind) -> Filter {
    Filter {
        kind,
        cutoff: 1000.0,
        damping: SQRT_2,
        root: 1.0,
    }
}

// root for a shelf or bell gain in dB: 10^(dB / 80)
fn with_db(kind: Kind, db: f32) -> Filter {
    Filter {
        root: 10f32.powf(db / 80.0),
        ..at(kind)
    }
}

/// Magnitude in dB at each frequency, from the filter's impulse response.
fn response_db(filter: Filter, hz: &[f32]) -> Vec<f32> {
    let mut svf = Svf::new(filter, FS);
    let mut input: Vec<f32> = (0..N)
        .map(|i| svf.next(if i == 0 { 1.0 } else { 0.0 }))
        .collect();
    let fft = realfft::RealFftPlanner::<f32>::new().plan_fft_forward(N);
    let mut spectrum = fft.make_output_vec();
    fft.process(&mut input, &mut spectrum).unwrap();
    hz.iter()
        .map(|&f| {
            let bin = (f * N as f32 / FS).round() as usize;
            20.0 * spectrum[bin].norm().log10()
        })
        .collect()
}

#[track_caller]
fn near(got: f32, want: f32, tolerance: f32) {
    assert!(
        (got - want).abs() <= tolerance,
        "got {got} dB, want {want} ± {tolerance}"
    );
}

#[track_caller]
fn below(got: f32, ceiling: f32) {
    assert!(got <= ceiling, "got {got} dB, want at most {ceiling}");
}

// deterministic white noise in -1..1, so failures reproduce
fn noise() -> impl FnMut() -> f32 {
    let mut state: u32 = 0x1234_5678;
    move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1 << 23) as f32 - 1.0
    }
}

#[test]
fn lowpass_response() {
    let db = response_db(at(Kind::Lowpass), &[100.0, 1000.0, 8000.0]);
    near(db[0], 0.0, 0.5);
    near(db[1], -3.0, 0.5);
    below(db[2], -33.0);
}

#[test]
fn highpass_response() {
    let db = response_db(at(Kind::Highpass), &[10000.0, 1000.0, 125.0]);
    near(db[0], 0.0, 0.5);
    near(db[1], -3.0, 0.5);
    below(db[2], -33.0);
}

#[test]
fn bandpass_peaks_at_unity() {
    let db = response_db(at(Kind::Bandpass), &[1000.0, 125.0, 8000.0]);
    near(db[0], 0.0, 0.5);
    below(db[1], -14.0);
    below(db[2], -14.0);
}

#[test]
fn notch_removes_cutoff() {
    let db = response_db(at(Kind::Notch), &[1000.0, 125.0]);
    below(db[0], -30.0);
    near(db[1], 0.0, 1.0);
}

#[test]
fn bell_boosts_cutoff() {
    let db = response_db(with_db(Kind::Bell, 6.0), &[1000.0, 50.0, 15000.0]);
    near(db[0], 6.0, 0.5);
    near(db[1], 0.0, 0.5);
    near(db[2], 0.0, 0.5);
}

#[test]
fn low_shelf_boost_and_cut() {
    let boost = response_db(with_db(Kind::LowShelf, 6.0), &[50.0, 15000.0]);
    near(boost[0], 6.0, 0.5);
    near(boost[1], 0.0, 0.5);
    let cut = response_db(with_db(Kind::LowShelf, -6.0), &[50.0]);
    near(cut[0], -6.0, 0.5);
}

#[test]
fn high_shelf_boost_and_cut() {
    let boost = response_db(with_db(Kind::HighShelf, 6.0), &[15000.0, 50.0]);
    near(boost[0], 6.0, 0.5);
    near(boost[1], 0.0, 0.5);
    let cut = response_db(with_db(Kind::HighShelf, -6.0), &[15000.0]);
    near(cut[0], -6.0, 0.5);
}

#[test]
fn cutoff_clamps_below_nyquist() {
    let mut svf = Svf::new(lowpass(30000.0, SQRT_2), FS);
    assert_eq!(svf.cutoff(), 0.49 * FS);
    let mut white = noise();
    for _ in 0..FS as usize {
        assert!(svf.next(white()).is_finite());
    }
}

#[test]
fn min_damping_rings_then_decays() {
    let mut svf = Svf::new(lowpass(1000.0, MIN_DAMPING), FS);
    let out: Vec<f32> = (0..N)
        .map(|i| svf.next(if i == 0 { 1.0 } else { 0.0 }))
        .collect();
    assert!(out.iter().all(|y| y.is_finite()));
    let tail = out[N - 1000..].iter().fold(0.0f32, |m, y| m.max(y.abs()));
    assert!(tail < 1e-3, "still ringing at {tail}");
}

#[test]
fn cutoff_glides() {
    let mut svf = Svf::new(lowpass(100.0, SQRT_2), FS);
    svf.set_cutoff(20000.0);
    svf.next(0.0);
    assert!(svf.cutoff() < 200.0, "jumped to {}", svf.cutoff());
    for _ in 1..1200 {
        svf.next(0.0);
    }
    assert!(
        (svf.cutoff() - 20000.0).abs() < 200.0,
        "only reached {}",
        svf.cutoff()
    );
}

#[test]
fn hard_sweep_stays_bounded() {
    let mut svf = Svf::new(lowpass(20.0, 0.2), FS);
    for i in 0..FS as usize {
        if i % 512 == 0 {
            svf.set_cutoff(if (i / 512) % 2 == 0 { 20000.0 } else { 20.0 });
        }
        let x = 0.5 * (2.0 * core::f32::consts::PI * 440.0 * i as f32 / FS).sin();
        let y = svf.next(x);
        assert!(y.is_finite() && y.abs() < 4.0, "sample {i} is {y}");
    }
}

#[test]
fn silence_decays_to_exact_zero() {
    let mut svf = Svf::new(at(Kind::Lowpass), FS);
    let mut last = svf.next(1.0);
    for _ in 0..480_000 {
        last = svf.next(0.0);
    }
    assert_eq!(last, 0.0);
}

// at a sample rate whose ceiling falls under MIN_CUTOFF, the ceiling wins
#[test]
fn tiny_sample_rate_takes_the_ceiling() {
    let svf = Svf::new(lowpass(1000.0, SQRT_2), 4.0);
    assert_eq!(svf.cutoff(), 0.49 * 4.0);
}

#[test]
fn sin_turn_tracks_std() {
    for i in 0..1000 {
        let turn = i as f32 / 1000.0;
        let want = (2.0 * PI * turn).sin();
        let got = approx::sin_turn(turn);
        assert!(
            (got - want).abs() <= 2e-3,
            "sin_turn({turn}) = {got}, want {want}"
        );
    }
}

use super::lfo::{Lfo, Shape};

const SHAPES: [Shape; 4] = [Shape::Sine, Shape::Triangle, Shape::Saw, Shape::Square];

/// One cycle at 1 Hz over a 100 Hz rate: 100 samples.
fn cycle(shape: Shape) -> Vec<f32> {
    let mut lfo = Lfo::new(shape, 1.0, 100.0);
    (0..100).map(|_| lfo.next()).collect()
}

#[test]
fn every_lfo_shape_stays_within_full_scale() {
    for shape in SHAPES {
        assert!(
            cycle(shape).iter().all(|v| (-1.0..=1.0).contains(v)),
            "{shape:?}"
        );
    }
}

#[test]
fn an_lfo_peaks_where_its_shape_does() {
    let sine = cycle(Shape::Sine);
    assert!((sine[25] - 1.0).abs() < 2e-3 && (sine[75] + 1.0).abs() < 2e-3);
    // steps of 0.01 add up only close to 0.25, so read near the edges
    assert!((cycle(Shape::Triangle)[25] - 1.0).abs() < 1e-4);
    let square = cycle(Shape::Square);
    assert_eq!((square[40], square[60]), (1.0, -1.0));
}

#[test]
fn an_lfo_repeats_every_period() {
    let mut lfo = Lfo::new(Shape::Triangle, 1.0, 100.0);
    let first: Vec<f32> = (0..100).map(|_| lfo.next()).collect();
    let second: Vec<f32> = (0..100).map(|_| lfo.next()).collect();
    for (a, b) in first.iter().zip(&second) {
        assert!((a - b).abs() < 1e-4, "{a} then {b}");
    }
}

#[test]
fn an_lfo_with_a_rate_that_is_not_finite_holds_still() {
    let mut lfo = Lfo::new(Shape::Saw, f32::NAN, 100.0);
    assert!((0..10).all(|_| lfo.next() == 0.0));
}

#[test]
fn reset_returns_an_lfo_to_its_start() {
    let mut lfo = Lfo::new(Shape::Sine, 5.0, 100.0);
    (0..7).for_each(|_| {
        lfo.next();
    });
    lfo.reset();
    assert_eq!(lfo.next(), 0.0);
}
