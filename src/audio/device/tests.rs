use super::super::render::Feed;
use super::*;
use crate::spec::{Event, Freqs, Instrument, Segment, Timeline};

fn player() -> Player {
    let flat = || Segment {
        curve: vec![1.0],
        ms: 0.0,
    };
    let instrument = Instrument::new(
        vec![0.25, -0.5, 1.0, -1.0].into(),
        flat(),
        flat(),
        1.0,
        flat(),
        1,
    )
    .unwrap();
    let mut hz = [0.0; 128];
    hz[1] = 1.0;
    let on = Event::NoteOn {
        instrument: 0,
        note: 1,
        velocity: 127,
    };
    Player::new(
        Feed::Timeline(Timeline {
            instruments: vec![instrument],
            events: vec![(0, on)],
            total_samples: 4096,
        }),
        Freqs::unchecked(hz),
        4.0,
    )
}

// three channels do not divide SCRATCH, so a pass that split a frame
// would show here as a shifted or repeated sample
#[test]
fn convert_matches_fill_across_passes() {
    let channels = 3;
    let mut expected = vec![0.0f32; channels * 1000];
    let want = fill(&mut expected, channels, &mut player());

    let mut out = vec![0i16; expected.len()];
    let mut passes = Vec::new();
    let got = convert(&mut out, channels, &mut player(), &mut |events| {
        passes.push(events);
    });

    assert_eq!(got, want);
    let converted: Vec<i16> = expected.iter().map(|&s| i16::from_sample(s)).collect();
    assert_eq!(out, converted);
    assert_eq!(passes, vec![1, 0, 0]);
}

// a count, not a gauge: taking a snapshot must not reset it
#[test]
fn dropped_events_accumulate_across_snapshots() {
    let stats = Stats::default();
    stats.record_dropped();
    stats.record_dropped();
    assert_eq!(stats.snapshot().dropped, 2);
    assert_eq!(stats.snapshot().dropped, 2);
}

fn overrun() -> Tally {
    Tally {
        frames: 48,
        output_tee_dropped: 2,
        ..Tally::default()
    }
}

#[test]
fn an_overrun_and_a_full_output_tee_are_faults() {
    let stats = Stats::default();
    let mut faults = Faults::new(64);
    let mut tx = faults.ring();
    // 48 frames at 48 kHz is a 1000 µs deadline
    account(&stats, &mut tx, &overrun(), 1500, 48_000);
    faults.drain();
    assert_eq!(
        faults.last(),
        Some(Fault::Overrun {
            used_us: 1500,
            budget_us: 1000
        })
    );
    assert_eq!(stats.snapshot().overruns, 1);
}

#[test]
fn a_stream_error_keeps_its_kind() {
    let stats = Stats::default();
    let mut faults = Faults::new(64);
    let mut tx = faults.ring();
    stream_error(&stats, &mut tx, &cpal::Error::new(cpal::ErrorKind::Xrun));
    faults.drain();
    assert_eq!(faults.last(), Some(Fault::Stream(StreamKind::Underrun)));
    assert_eq!(stats.snapshot().errors, 1);
}

// a full ring loses the record, and the count still moves
#[test]
fn faults_never_allocate_whether_the_ring_has_room_or_is_full() {
    let stats = Stats::default();
    let mut faults = Faults::new(64);
    let mut tx = faults.ring();
    let tally = overrun();
    let error = cpal::Error::new(cpal::ErrorKind::Xrun);
    let room = tx.slots();
    let info = allocation_counter::measure(|| {
        for _ in 0..room {
            account(&stats, &mut tx, &tally, 1500, 48_000);
            stream_error(&stats, &mut tx, &error);
        }
    });
    assert_eq!(info.count_total, 0);
    assert_eq!(tx.slots(), 0);
    assert_eq!(stats.snapshot().overruns, room as u64);
    assert_eq!(stats.snapshot().errors, room as u64);
}

const RANGE: cpal::SupportedBufferSize = cpal::SupportedBufferSize::Range { min: 64, max: 4096 };

#[test]
fn no_setting_leaves_the_buffer_to_the_platform() {
    assert_eq!(
        buffer_size(None, &RANGE).unwrap(),
        cpal::BufferSize::Default
    );
}

#[test]
fn a_size_in_range_is_asked_for() {
    assert_eq!(
        buffer_size(Some(128), &RANGE).unwrap(),
        cpal::BufferSize::Fixed(128)
    );
}

#[test]
fn a_size_outside_the_device_range_is_named() {
    let error = buffer_size(Some(32), &RANGE).unwrap_err();
    assert!(
        matches!(
            error,
            Error::BufferFrames {
                frames: 32,
                min: 64,
                max: 4096
            }
        ),
        "{error}"
    );
}

#[test]
fn a_device_with_no_range_gets_the_request() {
    let unknown = cpal::SupportedBufferSize::Unknown;
    assert_eq!(
        buffer_size(Some(32), &unknown).unwrap(),
        cpal::BufferSize::Fixed(32)
    );
}
