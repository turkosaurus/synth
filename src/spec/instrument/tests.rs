use super::*;

fn stage(curve: Vec<f32>) -> Segment {
    Segment { curve, ms: 1.0 }
}

fn build(wave: Wavetable, attack: Vec<f32>) -> Result<Instrument, InstrumentError> {
    Instrument::new(
        wave,
        stage(attack),
        stage(vec![0.0, 1.0]),
        0.5,
        stage(vec![0.0, 1.0]),
        1,
    )
}

fn table(frames: Vec<f32>, points: usize) -> Wavetable {
    Wavetable {
        frames,
        points,
        slopes: Vec::new(),
        position: 0.0,
        position_mod: None,
    }
}

#[test]
fn a_clean_instrument_builds() {
    assert!(build(vec![0.0, 1.0, 0.0, -1.0].into(), vec![0.0, 1.0]).is_ok());
}

#[test]
fn an_empty_wave_is_refused() {
    let err = build(Vec::new().into(), vec![0.0, 1.0]).err();
    assert_eq!(err, Some(InstrumentError::NoFrames));
    let err = build(table(Vec::new(), 4), vec![0.0, 1.0]).err();
    assert_eq!(err, Some(InstrumentError::NoFrames));
}

#[test]
fn every_frame_is_checked() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.5, -1.01] {
        let wave = table(vec![0.0, 1.0, 0.0, -1.0, 0.0, 1.0, bad, -1.0], 4);
        let err = build(wave, vec![0.0, 1.0]).err().expect("refused");
        let InstrumentError::Wave { frame, phase, .. } = err else {
            panic!("{err:?}");
        };
        assert_eq!((frame, phase), (1, 0.5), "for {bad}");
    }
}

fn swept(slopes: Vec<Vec<f32>>, position: f32, cycle: Vec<f32>) -> Wavetable {
    Wavetable {
        slopes,
        position,
        position_mod: Some(Lfo {
            cycle,
            rate: 1.0,
            depth: 0.5,
        }),
        ..table(vec![0.0, 1.0, 0.0, -1.0], 2)
    }
}

#[test]
fn slopes_position_and_lfo_are_checked() {
    let ok = || swept(vec![vec![0.0, 1.0]], 0.5, vec![0.0, 1.0]);
    assert!(build(ok(), vec![0.0, 1.0]).is_ok());
    for bad in [f32::NAN, f32::INFINITY, 1.5, -0.5] {
        let err = build(
            swept(vec![vec![0.0, 1.0], vec![0.0, bad, 1.0]], 0.5, vec![0.0]),
            vec![0.0, 1.0],
        )
        .err();
        assert!(
            matches!(err, Some(InstrumentError::Slope { index: 1, t, .. }) if t == 0.5),
            "{err:?}"
        );
        let err = build(swept(vec![], bad, vec![0.0]), vec![0.0, 1.0]).err();
        assert!(
            matches!(err, Some(InstrumentError::Position { .. })),
            "{err:?}"
        );
    }
    for bad in [f32::NAN, f32::NEG_INFINITY, 1.5] {
        let err = build(swept(vec![], 0.5, vec![0.0, bad]), vec![0.0, 1.0]).err();
        assert!(
            matches!(err, Some(InstrumentError::Lfo { phase, .. }) if phase == 0.5),
            "{err:?}"
        );
    }
}

fn lfo(rate: f32, depth: f32) -> Wavetable {
    Wavetable {
        position_mod: Some(Lfo {
            cycle: vec![0.0, 1.0],
            rate,
            depth,
        }),
        ..table(vec![0.0, 1.0, 0.0, -1.0], 2)
    }
}

#[test]
fn lfo_rate_and_depth_are_checked() {
    assert!(build(lfo(0.0, 0.0), vec![0.0, 1.0]).is_ok());
    assert!(build(lfo(20.0, 1.0), vec![0.0, 1.0]).is_ok());
    for bad in [f32::NAN, f32::INFINITY, -0.1] {
        let err = build(lfo(bad, 0.5), vec![0.0, 1.0]).err();
        assert!(
            matches!(err, Some(InstrumentError::LfoRate { .. })),
            "{err:?} for {bad}"
        );
    }
    for bad in [f32::NAN, f32::INFINITY, -0.1, 1.5] {
        let err = build(lfo(1.0, bad), vec![0.0, 1.0]).err();
        assert!(
            matches!(err, Some(InstrumentError::LfoDepth { .. })),
            "{err:?} for {bad}"
        );
    }
}

#[test]
fn every_time_is_checked() {
    let timed = |ms: [f64; 3]| {
        let segment = |ms| Segment {
            curve: vec![0.0, 1.0],
            ms,
        };
        Instrument::new(
            vec![0.0, 1.0].into(),
            segment(ms[0]),
            segment(ms[1]),
            0.5,
            segment(ms[2]),
            1,
        )
    };
    assert!(timed([0.0, 0.0, 0.0]).is_ok());
    for bad in [f64::NAN, f64::INFINITY, -1.0] {
        let err = timed([1.0, 1.0, bad]).err();
        assert!(
            matches!(
                err,
                Some(InstrumentError::Time {
                    stage: "release",
                    ..
                })
            ),
            "{err:?} for {bad}"
        );
    }
}

#[test]
fn no_voices_is_refused() {
    let err = Instrument::new(
        vec![0.0, 1.0].into(),
        stage(vec![0.0, 1.0]),
        stage(vec![0.0, 1.0]),
        0.5,
        stage(vec![0.0, 1.0]),
        0,
    )
    .err();
    assert_eq!(err, Some(InstrumentError::NoVoices));
}

#[test]
fn every_note_needs_a_positive_finite_pitch() {
    let freqs = Freqs::new([440.0; 128]).expect("clean");
    assert_eq!(freqs.hz(127), Some(440.0));
    assert_eq!(freqs.hz(128), None);
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut hz = [440.0; 128];
        hz[60] = bad;
        let err = Freqs::new(hz).err();
        assert!(
            matches!(err, Some(FreqsError::Hz { note: 60, .. })),
            "{err:?} for {bad}"
        );
    }
}

#[test]
fn every_envelope_point_is_checked() {
    for bad in [f32::NAN, f32::INFINITY, 1000.0, -0.1] {
        let err = build(vec![0.0, 1.0].into(), vec![0.0, bad, 1.0]).err();
        let Some(InstrumentError::Envelope { stage, t, .. }) = err else {
            panic!("{err:?} for {bad}");
        };
        assert_eq!((stage, t), ("attack", 0.5), "for {bad}");
    }
}
