use super::*;
use crate::spec::{Kind, Lfo, Segment, Wavetable};

/// A stage that plays `along` one point per sample at the 4 Hz test
/// rate, so each sample reads a point exactly.
fn stage(along: &[f32]) -> Segment {
    let mut curve = along.to_vec();
    curve.push(1.0);
    Segment {
        curve,
        ms: along.len() as f64 * 250.0,
    }
}

/// What `Instrument::new` takes, so a test can change one part.
struct Parts {
    wave: Wavetable,
    attack: Segment,
    decay: Segment,
    sustain_level: f32,
    release: Segment,
    voice_count: usize,
}

impl Parts {
    fn build(self) -> Instrument {
        Instrument::new(
            self.wave,
            self.attack,
            self.decay,
            self.sustain_level,
            self.release,
            self.voice_count,
        )
        .unwrap()
    }

    fn unchecked(self) -> Instrument {
        Instrument::unchecked(
            self.wave,
            self.attack,
            self.decay,
            self.sustain_level,
            self.release,
            self.voice_count,
        )
    }
}

// 4-entry wave so interpolation is checkable by hand
fn parts() -> Parts {
    Parts {
        wave: vec![0.0, 1.0, 0.0, -1.0].into(),
        attack: stage(&[0.0, 0.5]),
        decay: stage(&[0.0, 0.5]), // 1.0, 0.85
        sustain_level: 0.7,
        release: stage(&[0.0, 0.5]), // 0.7, 0.35
        voice_count: 1,
    }
}

fn instrument() -> Instrument {
    parts().build()
}

// 4 Hz sample rate makes hold = 1s exactly 4 samples
fn note(instrument: Instrument) -> Note {
    Note::new(
        Arc::new(instrument),
        1.0,
        1.0,
        4.0,
        Some(Duration::from_secs(1)),
    )
}

/// Frames that each hold one level, so a read at any phase shows the
/// blend directly.
fn table(levels: &[f32], slopes: Vec<Vec<f32>>, position: f32) -> Instrument {
    table_parts(levels, slopes, position).build()
}

fn table_parts(levels: &[f32], slopes: Vec<Vec<f32>>, position: f32) -> Parts {
    Parts {
        wave: Wavetable {
            frames: levels.iter().flat_map(|&level| [level; 4]).collect(),
            points: 4,
            slopes,
            position,
            position_mod: None,
        },
        ..parts()
    }
}

/// Two frames a blend apart, swept by an LFO whose cycle reads
/// 0, 0.5, 1, -1 at one entry per sample.
fn swept(voice_count: usize) -> Pool {
    let mut parts = table_parts(&[0.0, 1.0], vec![], 0.5);
    parts.voice_count = voice_count;
    parts.wave.position_mod = Some(Lfo {
        cycle: vec![0.0, 0.5, 1.0, -1.0],
        rate: 1.0,
        depth: 0.5,
    });
    Pool::new(Arc::new(parts.build()))
}

fn start(pool: &mut Pool, slot: usize, pitch: u8) {
    pool.voices[slot] = Some(Voice {
        note: Note::new(Arc::clone(&pool.instrument), 1.0, 1.0, 4.0, None),
        pitch,
        age: 0,
    });
}

fn mix(pool: &Pool, slot: usize) -> f32 {
    pool.voices[slot].as_ref().unwrap().note.mix
}

#[test]
fn an_lfo_moves_a_note_through_the_table() {
    let mut pool = swept(1);
    start(&mut pool, 0, 60);
    // position 0.5 + 0.5 × (0, 0.5, 1): the blend follows each sample
    let got: Vec<f32> = (0..3)
        .map(|_| {
            pool.next();
            mix(&pool, 0)
        })
        .collect();
    assert_eq!(got, [0.5, 0.75, 1.0]);
}

#[test]
fn every_note_starts_its_own_sweep() {
    let mut pool = swept(2);
    start(&mut pool, 0, 60);
    pool.next();
    pool.next();
    // the second note starts two samples after the first
    start(&mut pool, 1, 64);
    pool.next();
    assert_eq!(
        mix(&pool, 1),
        0.5,
        "the new note is at the start of its sweep"
    );
    assert_eq!(mix(&pool, 0), 1.0, "the first note kept its own place");
}

#[test]
fn a_table_sounds_its_end_frames_at_position_0_and_1() {
    assert_eq!(note(table(&[0.2, 0.8], vec![], 0.0)).wave(), 0.2);
    assert_eq!(note(table(&[0.2, 0.8], vec![], 1.0)).wave(), 0.8);
}

#[test]
fn a_table_blends_linearly_with_no_slopes() {
    let level = note(table(&[0.2, 0.8], vec![], 0.5)).wave();
    assert!((level - 0.5).abs() < 1e-6, "{level}");
}

#[test]
fn a_slope_shapes_the_blend() {
    // t squared at three points: halfway along is a quarter of the way
    let level = note(table(&[0.0, 1.0], vec![vec![0.0, 0.25, 1.0]], 0.5)).wave();
    assert!((level - 0.25).abs() < 1e-6, "{level}");
}

#[test]
fn a_position_picks_the_transition_it_falls_in() {
    // three frames make two transitions; 0.75 is halfway through the second
    let level = note(table(&[0.0, 0.4, 1.0], vec![], 0.75)).wave();
    assert!((level - 0.7).abs() < 1e-6, "{level}");
}

#[test]
fn envelope_walks_every_stage_in_order() {
    let mut note = note(instrument());
    let got: Vec<f32> = (0..12).map(|_| note.envelope()).collect();
    assert_eq!(
        got,
        vec![
            0.0, 0.5, // attack
            1.0, 0.85, // decay
            0.7, 0.7, 0.7, 0.7, // sustain, held 4 samples
            0.7, 0.35, // release
            0.0, 0.0, // done
        ]
    );
}

#[test]
fn a_stage_longer_than_its_curve_reads_between_points() {
    // two points stretched over 1 s, 4 samples at 4 Hz
    let instrument = Parts {
        attack: Segment {
            curve: vec![0.0, 1.0],
            ms: 1000.0,
        },
        ..parts()
    };
    let mut note = note(instrument.build());
    let got: Vec<f32> = (0..4).map(|_| note.envelope()).collect();
    assert_eq!(got, [0.0, 0.25, 0.5, 0.75]);
}

#[test]
fn zero_length_segment_falls_through() {
    let instrument = Parts {
        attack: stage(&[]),
        ..parts()
    };
    // with no attack samples the first value must come from decay
    assert_eq!(note(instrument.build()).envelope(), 1.0);
}

#[test]
fn release_resumes_from_the_current_level() {
    let mut note = note(instrument());
    assert_eq!(note.envelope(), 0.0);
    assert_eq!(note.envelope(), 0.5);

    // lifted mid-attack: release must start at 0.5, not jump to sustain
    note.off();
    assert!((note.envelope() - 0.5).abs() < 1e-6);
}

// a voice stolen mid-release hands its level to the next note, so the
// level has to follow the release down rather than stay at sustain
#[test]
fn release_tracks_the_falling_level() {
    let mut note = note(instrument());
    for _ in 0..5 {
        note.envelope(); // attack, decay, into sustain
    }
    note.off();

    assert_eq!(note.envelope(), 0.7);
    assert_eq!(note.envelope(), 0.35);
    assert_eq!(note.level, 0.35);

    note.envelope(); // release exhausted
    assert_eq!(note.level, 0.0);
}

#[test]
fn wave_interpolates_and_wraps() {
    let mut note = note(instrument());

    note.phase = 0.0;
    assert_eq!(note.wave(), 0.0);

    note.phase = 0.125; // halfway between wave[0] and wave[1]
    assert_eq!(note.wave(), 0.5);

    note.phase = 0.25;
    assert_eq!(note.wave(), 1.0);

    note.phase = 0.875; // halfway from wave[3] back around to wave[0]
    assert_eq!(note.wave(), -0.5);
}

// an instrument whose envelope holds 1.0 forever, so fill() output is the
// waveform alone and easy to predict
fn flat() -> Parts {
    Parts {
        wave: vec![0.25, 0.5, 0.75, 1.0].into(),
        attack: stage(&[]),
        decay: stage(&[]),
        sustain_level: 1.0,
        release: stage(&[]),
        voice_count: 1,
    }
}

fn flat_instrument() -> Instrument {
    flat().build()
}

// a Player whose timeline is a single note-on at sample 0
// freqs[1] = 1.0 so step is 1/4 of the table per sample at rate 4.0
fn freqs() -> Freqs {
    Freqs::unchecked(hz())
}

fn hz() -> [f32; 128] {
    let mut hz = [0.0; 128];
    hz[1] = 1.0;
    hz
}

fn live(instruments: Vec<Instrument>, events: rtrb::Consumer<Event>) -> Feed {
    Feed::Live {
        instruments,
        events,
    }
}

fn player(instrument: Instrument) -> Player {
    Player::new(
        Feed::Timeline(Timeline {
            instruments: vec![instrument],
            events: vec![(
                0,
                Event::NoteOn {
                    instrument: 0,
                    note: 1,
                    velocity: 127,
                },
            )],
            total_samples: 64,
        }),
        freqs(),
        4.0,
    )
}

// the flat instrument's wave sits above zero, so a highpass, which
// removes that offset, must leave the output quieter on average
#[test]
fn filter_shapes_player_output() {
    let level = |filter: Option<Filter>| {
        let mut player = player(flat_instrument()).with_filter(filter);
        let out: Vec<f32> = (0..64).map(|_| player.next()).collect();
        out[32..].iter().map(|y| y.abs()).sum::<f32>()
    };
    let highpass = Filter {
        kind: Kind::Highpass,
        cutoff: 20000.0, // clamps just below Nyquist
        damping: core::f32::consts::SQRT_2,
        root: 1.0,
        cutoff_mod: None,
    };
    let (dry, wet) = (level(None), level(Some(highpass)));
    assert!(wet < dry, "filtered {wet} is not below unfiltered {dry}");
}

/// A live player at 48 kHz whose lowpass at 1000 Hz is moved by an
/// LFO with `cycle`, at one entry per sample.
fn swept_filter(cycle: Vec<f32>, depth: f32, unit: Unit) -> (rtrb::Producer<Event>, Player) {
    let (tx, rx) = rtrb::RingBuffer::new(4);
    let lowpass = Filter {
        kind: Kind::Lowpass,
        cutoff: 1000.0,
        damping: core::f32::consts::SQRT_2,
        root: 1.0,
        cutoff_mod: Some(CutoffMod {
            lfo: Lfo {
                rate: 48000.0 / cycle.len() as f32,
                cycle,
                depth,
            },
            unit,
        }),
    };
    let player =
        Player::new(live(vec![flat_instrument()], rx), freqs(), 48000.0).with_filter(Some(lowpass));
    (tx, player)
}

fn cutoff(player: &Player) -> f32 {
    player.filter.as_ref().unwrap().cutoff()
}

#[test]
fn cutoff_mod_moves_the_cutoff_in_its_unit_and_glides() {
    for (cycle, depth, unit, want) in [
        (vec![-1.0, -1.0], 300.0, Unit::Hz, 700.0),
        (vec![1.0, 1.0], 1.0, Unit::Octaves, 2000.0),
    ] {
        let (_tx, mut player) = swept_filter(cycle, depth, unit);
        player.next();
        let first = cutoff(&player);
        assert!((first - 1000.0).abs() < 50.0, "{unit:?} jumped to {first}");
        for _ in 0..48000 {
            player.next();
        }
        let settled = cutoff(&player);
        assert!(
            (settled - want).abs() < 1.0,
            "{unit:?} settled at {settled}"
        );
    }
}

#[test]
fn a_note_on_does_not_restart_the_cutoff_lfo() {
    let cycle = vec![0.0, 0.5, 1.0, 0.5, 0.0, -0.5, -1.0, -0.5];
    let (_quiet_tx, mut quiet) = swept_filter(cycle.clone(), 1.0, Unit::Octaves);
    let (mut tx, mut played) = swept_filter(cycle, 1.0, Unit::Octaves);
    for i in 0..64 {
        if i == 13 {
            tx.push(on(1)).unwrap();
        }
        quiet.next();
        played.next();
        assert_eq!(cutoff(&quiet), cutoff(&played), "sample {i}");
    }
}

#[test]
fn a_live_feed_drains_the_queue() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);

    // nothing queued yet, so silence
    assert_eq!(player.next(), 0.0);

    tx.push(Event::NoteOn {
        instrument: 0,
        note: 1,
        velocity: 127,
    })
    .unwrap();
    assert_eq!(player.next(), 0.25); // wave[0], envelope held at 1.0

    tx.push(Event::NoteOff {
        instrument: 0,
        note: 1,
    })
    .unwrap();
    // release is empty in flat_instrument, so the note goes straight to Done
    assert_eq!(player.next(), 0.0);
}

#[test]
fn a_noteoff_for_a_note_that_is_not_sounding_is_ignored() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);

    tx.push(Event::NoteOn {
        instrument: 0,
        note: 1,
        velocity: 127,
    })
    .unwrap();
    assert_eq!(player.next(), 0.25);

    // a stale release from a key that was already stolen must not cut the
    // note that is actually sounding
    tx.push(Event::NoteOff {
        instrument: 0,
        note: 99,
    })
    .unwrap();
    assert_eq!(player.next(), 0.5); // still playing, advanced one entry
}

#[test]
fn velocity_zero_means_note_off() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);

    tx.push(Event::NoteOn {
        instrument: 0,
        note: 1,
        velocity: 127,
    })
    .unwrap();
    assert_eq!(player.next(), 0.25);

    // the MIDI convention: a note-on at velocity 0 releases
    tx.push(Event::NoteOn {
        instrument: 0,
        note: 1,
        velocity: 0,
    })
    .unwrap();
    assert_eq!(player.next(), 0.0);
}

#[test]
fn velocity_scales_the_output() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);

    tx.push(Event::NoteOn {
        instrument: 0,
        note: 1,
        velocity: 64,
    })
    .unwrap();
    // wave[0] is 0.25, envelope 1.0, velocity 64/127
    let expected = 0.25 * (64.0 / 127.0);
    assert!((player.next() - expected).abs() < 1e-6);
}

#[test]
fn a_retrigger_continues_the_waveform() {
    let (mut tx, mut player) = pooled(1);
    tx.push(on(1)).unwrap();
    player.next();
    player.next(); // advance into the table

    // the retrigger keeps the phase, so this is wave[2], not wave[0]
    tx.push(on(1)).unwrap();
    assert_eq!(player.next(), 0.75);
}

fn rising() -> Instrument {
    Parts {
        attack: stage(&[0.0, 0.25, 0.5, 0.75]),
        ..parts()
    }
    .build()
}

#[test]
fn take_over_resumes_attack_at_the_handed_over_level() {
    let mut from = Note::new(Arc::new(rising()), 1.0, 1.0, 4.0, None);
    from.level = 0.5;
    from.phase = 0.3;

    let mut note = Note::new(Arc::new(rising()), 2.0, 1.0, 4.0, None);
    note.take_over(&from);
    assert_eq!(note.phase, 0.3);
    assert_eq!(note.envelope(), 0.5); // attack[2], not attack[0]
}

#[test]
fn take_over_matches_audible_level_across_velocities() {
    let mut from = Note::new(Arc::new(rising()), 1.0, 0.5, 4.0, None);
    from.level = 0.5; // audible at 0.5 × 0.5 = 0.25

    let mut note = Note::new(Arc::new(rising()), 1.0, 1.0, 4.0, None);
    note.take_over(&from);
    assert_eq!(note.envelope(), 0.25); // 0.25 × 1.0 matches
}

#[test]
fn a_soft_note_stealing_a_loud_one_eases_down() {
    let mut from = Note::new(Arc::new(rising()), 1.0, 1.0, 4.0, None);
    from.level = 0.7; // sustaining at full velocity
    from.phase = 0.25; // wave[1] = 1.0, so samples read as env × velocity

    let soft = 40.0 / 127.0;
    let mut note = Note::new(Arc::new(rising()), 0.0, soft, 4.0, None);
    note.take_over(&from);

    // 0.7 is above anything the soft note's envelope can reach, so
    // velocity carries it; the first sample matches, with no drop
    assert!((note.next() - 0.7).abs() < 1e-6);
    // GLIDE is under one sample at this rate, so it lands at once
    assert_eq!(note.velocity, soft);
}

#[test]
fn take_over_with_no_attack_eases_up_from_the_handed_over_level() {
    let instrument = Parts {
        attack: stage(&[]),
        ..parts()
    }
    .build();
    let mut from = Note::new(Arc::new(rising()), 1.0, 1.0, 4.0, None);
    from.level = 0.2;
    from.phase = 0.25;

    let mut note = Note::new(Arc::new(instrument), 0.0, 1.0, 4.0, None);
    note.take_over(&from);
    // decay starts at 1.0, so velocity starts at 0.2 instead of jumping
    assert!((note.next() - 0.2).abs() < 1e-6);
    assert_eq!(note.velocity, 1.0);
}

// A live player on flat_instrument with `voice_count` voices. Only
// freqs[1] is nonzero, so any other note holds still at wave[0] = 0.25.
fn pooled(voice_count: usize) -> (rtrb::Producer<Event>, Player) {
    let (tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let instrument = Parts {
        voice_count,
        ..flat()
    };
    (
        tx,
        Player::new(live(vec![instrument.build()], rx), freqs(), 4.0),
    )
}

fn on(note: u8) -> Event {
    Event::NoteOn {
        instrument: 0,
        note,
        velocity: 127,
    }
}

fn off(note: u8) -> Event {
    Event::NoteOff {
        instrument: 0,
        note,
    }
}

fn pitches(player: &Player) -> Vec<Option<u8>> {
    let voices = &player.pools[0].voices;
    voices.iter().map(|v| v.as_ref().map(|v| v.pitch)).collect()
}

#[test]
fn a_pool_is_scaled_by_one_over_root_voice_count() {
    let (mut tx, mut player) = pooled(4);
    tx.push(on(2)).unwrap();
    assert_eq!(player.next(), 0.25 * 0.5);
}

#[test]
fn two_pitches_sound_together() {
    let (mut tx, mut player) = pooled(2);
    tx.push(on(1)).unwrap();
    player.next();

    tx.push(on(2)).unwrap();
    // note 1 has moved on to wave[1]; note 2 adds its wave[0]
    let gain = 1.0 / 2.0f32.sqrt();
    assert!((player.next() - (0.5 + 0.25) * gain).abs() < 1e-6);
    assert_eq!(pitches(&player), vec![Some(1), Some(2)]);
}

#[test]
fn a_retrigger_reuses_its_voice_with_room_to_spare() {
    let (mut tx, mut player) = pooled(2);
    tx.push(on(1)).unwrap();
    player.next();
    tx.push(on(1)).unwrap();
    player.next();
    assert_eq!(pitches(&player), vec![Some(1), None]);
}

#[test]
fn a_second_release_does_not_restart_the_fade() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let instrument = Parts {
        release: stage(&[0.0, 0.5, 0.75]), // 1.0, 0.5, 0.25
        ..flat()
    };
    let mut player = Player::new(live(vec![instrument.build()], rx), freqs(), 4.0);

    tx.push(on(2)).unwrap();
    player.next();
    tx.push(off(2)).unwrap();
    assert_eq!(player.next(), 0.25); // wave[0] × release[0]
    tx.push(off(2)).unwrap();
    assert_eq!(player.next(), 0.25 * 0.5); // release[1], not release[0]
}

#[test]
fn a_retrigger_while_releasing_resumes_from_the_fade() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let instrument = Parts {
        release: stage(&[0.0, 0.5, 0.75]), // 1.0, 0.5, 0.25
        ..flat()
    };
    let mut player = Player::new(live(vec![instrument.build()], rx), freqs(), 4.0);

    tx.push(on(2)).unwrap();
    player.next();
    tx.push(off(2)).unwrap();
    player.next(); // release[0]
    assert_eq!(player.next(), 0.25 * 0.5); // release[1]

    // the retrigger starts at the faded level, not full, then eases up
    tx.push(on(2)).unwrap();
    assert_eq!(player.next(), 0.25 * 0.5);
    assert_eq!(player.next(), 0.25);
    assert!(!player.pools[0].voices[0].as_ref().unwrap().note.releasing());
}

#[test]
fn a_full_pool_steals_the_oldest_voice() {
    let (mut tx, mut player) = pooled(2);
    for note in [1, 2, 3] {
        tx.push(on(note)).unwrap();
        player.next();
    }
    assert_eq!(pitches(&player), vec![Some(3), Some(2)]);

    // note 1 was stolen, so its release finds nothing to cut
    tx.push(off(1)).unwrap();
    player.next();
    assert_eq!(pitches(&player), vec![Some(3), Some(2)]);
}

#[test]
fn a_full_pool_steals_a_releasing_voice_before_an_older_held_one() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let instrument = Parts {
        voice_count: 2,
        release: stage(&[0.0; 16]), // long enough to still be fading
        ..flat()
    };
    let mut player = Player::new(live(vec![instrument.build()], rx), freqs(), 4.0);

    for event in [on(1), on(2), off(2), on(3)] {
        tx.push(event).unwrap();
        player.next();
    }
    assert_eq!(pitches(&player), vec![Some(1), Some(3)]);
}

#[test]
fn a_finished_voice_frees_its_slot() {
    let (mut tx, mut player) = pooled(2);
    tx.push(on(1)).unwrap();
    player.next();

    // flat_instrument has no release, so the note is done at once
    tx.push(off(1)).unwrap();
    player.next();
    assert_eq!(pitches(&player), vec![None, None]);
}

#[test]
fn mono_steals_on_every_new_pitch() {
    let (mut tx, mut player) = pooled(1);
    tx.push(on(1)).unwrap();
    player.next();
    // legato: note 2 takes over note 1's phase, now at wave[1], and
    // holds there because its own frequency is 0
    tx.push(on(2)).unwrap();
    assert_eq!(player.next(), 0.5);
    assert_eq!(player.next(), 0.5);
    assert_eq!(pitches(&player), vec![Some(2)]);
}

#[test]
fn all_notes_off_releases_every_voice_on_its_instrument() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let wide = Parts {
        voice_count: 2,
        ..flat()
    }
    .build();
    let instruments = vec![wide, flat_instrument()];
    let mut player = Player::new(live(instruments, rx), freqs(), 4.0);

    tx.push(on(1)).unwrap();
    tx.push(on(2)).unwrap();
    tx.push(Event::NoteOn {
        instrument: 1,
        note: 2,
        velocity: 127,
    })
    .unwrap();
    player.next();

    // flat_instrument has no release, so released voices free at once
    tx.push(Event::AllNotesOff { instrument: 0 }).unwrap();
    player.next();
    assert_eq!(pitches(&player), vec![None, None]);
    assert!(player.pools[1].voices[0].is_some(), "other instrument kept");
}

// a release stage that All Notes Off would play, so silence on the next
// sample shows it was skipped
#[test]
fn all_sound_off_silences_its_instrument_with_no_release() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let releasing = Parts {
        release: stage(&[0.0, 0.5, 0.75]),
        ..flat()
    }
    .build();
    let instruments = vec![releasing, flat_instrument()];
    let mut player = Player::new(live(instruments, rx), freqs(), 4.0);

    tx.push(on(1)).unwrap();
    tx.push(Event::NoteOn {
        instrument: 1,
        note: 2,
        velocity: 127,
    })
    .unwrap();
    player.next();

    tx.push(Event::AllSoundOff { instrument: 0 }).unwrap();
    player.next();
    assert_eq!(pitches(&player), vec![None]);
    assert!(player.pools[1].voices[0].is_some(), "other instrument kept");
}

#[test]
fn instruments_do_not_share_voices() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let instruments = vec![flat_instrument(), flat_instrument()];
    let mut player = Player::new(live(instruments, rx), freqs(), 4.0);

    tx.push(on(2)).unwrap();
    tx.push(Event::NoteOn {
        instrument: 1,
        note: 2,
        velocity: 127,
    })
    .unwrap();
    // each instrument is mono, but neither steals the other's voice
    assert_eq!(player.next(), 0.5);
}

fn drain(output_tee: &mut OutputTee) -> Vec<f32> {
    std::iter::from_fn(|| output_tee.samples.pop().ok()).collect()
}

#[test]
fn the_output_tee_gets_every_channel_of_every_frame() {
    let (tx, mut rx) = output_tee(4, 2);
    let mut player = player(flat_instrument()).with_output_tee(tx);
    let mut out = [0.0f32; 8];
    let tally = fill(&mut out, 2, &mut player);

    assert_eq!(tally.output_tee_dropped, 0);
    assert_eq!(rx.channels, 2);
    assert_eq!(drain(&mut rx), out);
}

// a reader that never reads must cost the callback frames, not time
#[test]
fn a_full_output_tee_drops_and_counts_frames() {
    let (tx, mut rx) = output_tee(3, 2);
    let mut player = player(flat_instrument()).with_output_tee(tx);
    let mut out = [0.0f32; 8];

    assert_eq!(fill(&mut out, 2, &mut player).output_tee_dropped, 1);
    assert_eq!(fill(&mut out, 2, &mut player).output_tee_dropped, 4);
    assert_eq!(drain(&mut rx), [0.25, 0.25, 0.5, 0.5, 0.75, 0.75]);
}

// five slots hold two stereo frames and one spare sample, which must
// stay empty or every frame after it reads with its channels swapped
#[test]
fn the_output_tee_never_takes_part_of_a_frame() {
    let (tx, samples) = rtrb::RingBuffer::new(5);
    let mut rx = OutputTee {
        samples,
        channels: 2,
    };
    let mut player = player(flat_instrument()).with_output_tee(tx);
    let mut out = [0.0f32; 8];

    assert_eq!(fill(&mut out, 2, &mut player).output_tee_dropped, 2);
    assert_eq!(drain(&mut rx), [0.25, 0.25, 0.5, 0.5]);
}

#[test]
fn fill_with_an_output_tee_never_allocates() {
    let (tx, _rx) = output_tee(3, 2);
    let mut player = player(flat_instrument()).with_output_tee(tx);
    let mut out = [0.0f32; 8];
    let info = allocation_counter::measure(|| {
        // room for three frames, then room for none
        fill(&mut out, 2, &mut player);
        fill(&mut out, 2, &mut player);
    });
    assert_eq!(info.count_total, 0);
}

#[test]
fn fill_writes_one_sample_across_every_channel() {
    let mut out = [0.0f32; 8];
    let tally = fill(&mut out, 2, &mut player(flat_instrument()));

    assert_eq!(tally.frames, 4);
    // each pair is one frame: both channels carry the same sample
    assert_eq!(out, [0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0]);
}

// splitting the buffer at each event must sound the same as reading the
// feed before every sample: events mid-buffer, two on one sample, one on
// a buffer's first sample, and one on its last
#[test]
fn fill_splits_the_buffer_at_each_timeline_event() {
    let on = |note| Event::NoteOn {
        instrument: 0,
        note,
        velocity: 127,
    };
    let off = |note| Event::NoteOff {
        instrument: 0,
        note,
    };
    let timeline = || Timeline {
        instruments: vec![flat_instrument()],
        events: vec![(1, on(1)), (3, off(1)), (3, on(2)), (5, off(2)), (8, on(1))],
        total_samples: 12,
    };
    let mut hz = hz();
    hz[2] = 2.0;
    let freqs = Freqs::unchecked(hz);

    let mut stepped = Player::new(Feed::Timeline(timeline()), freqs.clone(), 4.0);
    let expected: Vec<f32> = (0..12).map(|_| stepped.next()).collect();

    let mut split = Player::new(Feed::Timeline(timeline()), freqs, 4.0);
    let mut out = Vec::new();
    for frames in [5, 1, 3, 3] {
        let mut buffer = vec![9.0; frames];
        fill(&mut buffer, 1, &mut split);
        out.extend(buffer);
    }
    assert_eq!(out, expected);
}

// live events carry no time stamp, so all of them land on a callback's
// first frame
#[test]
fn fill_plays_live_events_from_the_first_frame() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);
    tx.push(Event::NoteOn {
        instrument: 0,
        note: 1,
        velocity: 127,
    })
    .unwrap();

    let mut out = [0.0f32; 4];
    fill(&mut out, 1, &mut player);
    assert_eq!(out, [0.25, 0.5, 0.75, 1.0]);
}

#[test]
fn fill_handles_mono_and_four_channels() {
    let mut mono = [0.0f32; 4];
    assert_eq!(fill(&mut mono, 1, &mut player(flat_instrument())).frames, 4);
    assert_eq!(mono, [0.25, 0.5, 0.75, 1.0]);

    let mut quad = [0.0f32; 8];
    assert_eq!(fill(&mut quad, 4, &mut player(flat_instrument())).frames, 2);
    assert_eq!(quad, [0.25, 0.25, 0.25, 0.25, 0.5, 0.5, 0.5, 0.5]);
}

#[test]
fn fill_tracks_peak_and_clipping() {
    let instrument = Parts {
        wave: vec![0.5, -2.0, 1.5, 0.1].into(),
        ..flat()
    }
    .unchecked();

    let mut out = [0.0f32; 8];
    let tally = fill(&mut out, 2, &mut player(instrument));

    assert_eq!(tally.frames, 4);
    // -2.0 and 1.5 are written at full scale
    assert_eq!(out, [0.5, 0.5, -1.0, -1.0, 1.0, 1.0, 0.1, 0.1]);
    assert_eq!(tally.clipped, 2); // -2.0 and 1.5 both exceed full scale
    assert_eq!(tally.peak, 2.0); // absolute and pre-clamp, so -2.0 wins
}

#[test]
fn fill_drops_non_finite_samples() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let instrument = Parts {
            wave: vec![0.5, bad, 0.25, 0.75].into(),
            ..flat()
        }
        .unchecked();

        let mut out = [0.0f32; 4];
        let tally = fill(&mut out, 1, &mut player(instrument));

        // wave[0] interpolates toward wave[1] with a weight of zero, and
        // a non-finite value times zero is NaN, so two samples are bad
        assert_eq!(out, [0.0, 0.0, 0.25, 0.75], "for {bad}");
        assert_eq!(tally.non_finite, 2, "for {bad}");
        assert_eq!(tally.clipped, 0, "for {bad}");
        assert_eq!(tally.peak, 0.75, "for {bad}");
    }
}

// Without the reset the filter state stays NaN, so every later sample
// would be dropped as well.
#[test]
fn the_filter_recovers_after_a_non_finite_sample() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let broken = Parts {
        wave: vec![0.5, f32::NAN, 0.25, 0.75].into(),
        ..flat()
    }
    .unchecked();
    let lowpass = Filter {
        kind: Kind::Lowpass,
        cutoff: 20000.0, // clamps just below Nyquist
        damping: core::f32::consts::SQRT_2,
        root: 1.0,
        cutoff_mod: None,
    };
    let instruments = vec![broken, flat_instrument()];
    let mut player = Player::new(live(instruments, rx), freqs(), 4.0).with_filter(Some(lowpass));

    tx.push(on(1)).unwrap();
    let mut out = [0.0f32; 4];
    assert_eq!(fill(&mut out, 1, &mut player).non_finite, 2);

    // the broken note reads NaN once more as it ends, and after that only
    // the clean instrument sounds
    tx.push(off(1)).unwrap();
    tx.push(Event::NoteOn {
        instrument: 1,
        note: 1,
        velocity: 127,
    })
    .unwrap();
    let mut out = [0.0f32; 8];
    let tally = fill(&mut out, 1, &mut player);

    assert_eq!(tally.non_finite, 1);
    assert_eq!(out[0], 0.0);
    for &y in &out[1..] {
        assert!(y.is_finite() && y != 0.0 && y.abs() <= CLIP, "{out:?}");
    }
}

#[test]
fn fill_clamps_summed_voices_to_full_scale() {
    let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
    let loud = || {
        Parts {
            wave: vec![1.0, -1.0, 1.0, -1.0].into(),
            ..flat()
        }
        .build()
    };
    let mut player = Player::new(live(vec![loud(), loud()], rx), freqs(), 4.0);

    // two mono instruments add with no scaling, so every sample is 2.0
    // or -2.0 before the clamp
    tx.push(on(1)).unwrap();
    tx.push(Event::NoteOn {
        instrument: 1,
        note: 1,
        velocity: 127,
    })
    .unwrap();
    let mut out = [0.0f32; 4];
    let tally = fill(&mut out, 1, &mut player);

    assert_eq!(out, [1.0, -1.0, 1.0, -1.0]);
    assert_eq!(tally.clipped, 4);
    assert_eq!(tally.peak, 2.0);
}

#[test]
fn fill_leaves_a_short_trailing_frame_partially_written() {
    // cpal should always hand over whole frames; this pins the behavior
    // if it ever does not
    let mut out = [0.0f32; 5];
    let tally = fill(&mut out, 2, &mut player(flat_instrument()));

    assert_eq!(tally.frames, 3); // two full frames plus a short one
    assert_eq!(out, [0.25, 0.25, 0.5, 0.5, 0.75]);
}

#[test]
fn phase_advances_by_frequency_over_sample_rate() {
    let mut note = Note::new(Arc::new(instrument()), 440.0, 1.0, 48000.0, None);
    assert!((note.step - 440.0 / 48000.0).abs() < 1e-9);

    let before = note.phase;
    note.next();
    assert!((note.phase - before - note.step).abs() < 1e-6);
}
