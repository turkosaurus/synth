use crate::audio::Instrument;

use super::bake::inspect;
use super::*;
use std::path::Path;

fn wave_from(f: impl Fn(f32) -> f32, points: usize) -> Vec<f32> {
    (0..points)
        .map(|i| f(i as f32 / points as f32).clamp(-1.0, 1.0))
        .collect()
}

fn triangle(phase: f32) -> f32 {
    if phase < 0.25 {
        4.0 * phase
    } else if phase < 0.75 {
        2.0 - 4.0 * phase
    } else {
        4.0 * phase - 4.0
    }
}

fn instrument_with(wave: Vec<f32>) -> Instrument {
    Instrument {
        wave,
        attack: vec![0.0, 0.5],
        decay: vec![1.0],
        sustain_level: 0.7,
        release: vec![0.7, 0.35],
    }
}

#[test]
fn clean_triangle_raises_nothing() {
    let report = inspect(&instrument_with(wave_from(triangle, 2048)), 0);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.peak, 1.0);
    assert!(report.dc.abs() < 1e-6);
}

#[test]
fn flags_clamping() {
    let report = inspect(&instrument_with(wave_from(triangle, 2048)), 17);
    assert!(report.warnings.iter().any(|w| w.contains("17 points")));
}

#[test]
fn flags_dc_offset() {
    // a triangle lifted entirely above zero
    let report = inspect(
        &instrument_with(wave_from(|p| triangle(p) * 0.4 + 0.5, 2048)),
        0,
    );
    assert!(report.warnings.iter().any(|w| w.contains("DC offset")));
}

#[test]
fn flags_wrap_discontinuity() {
    // a saw ends at +1 and restarts at -1: a step of 2.0 every cycle
    let report = inspect(&instrument_with(wave_from(|p| 2.0 * p - 1.0, 2048)), 0);
    assert!(report.warnings.iter().any(|w| w.contains("wrap smoothly")));
}

#[test]
fn flags_weak_peak() {
    let report = inspect(&instrument_with(wave_from(|p| triangle(p) * 0.2, 2048)), 0);
    assert!(report.warnings.iter().any(|w| w.contains("peaks at only")));
}

#[test]
fn flags_zero_release_and_bad_sustain() {
    let mut instrument = instrument_with(wave_from(triangle, 2048));
    instrument.release = vec![];
    instrument.sustain_level = 1.5;
    let report = inspect(&instrument, 0);
    assert!(report.warnings.iter().any(|w| w.contains("releaseMS = 0")));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("outside 0.0..1.0")));
}

// 48000 Hz at 60 bpm makes one beat exactly 48000 samples
fn seq(steps: &str) -> Patch {
    let source = format!(
        r#"
        local sequence = require("synth.sequence")
        local instrument = require("synth.instrument")
        local wave = require("synth.wave")
        local envelope = require("synth.envelope")
        return sequence.new({{
            bpm = 60,
            instruments = {{ a = instrument.new({{}}), b = instrument.new({{}}) }},
            steps = {steps},
        }})
        "#
    );
    load_source(&source, "test", 48000).unwrap()
}

#[test]
fn steps_land_one_beat_apart() {
    let patch = seq(r#"{ { note = 69, instrument = "a" }, {}, { note = 69 } }"#);
    let ons: Vec<u64> = patch
        .timeline
        .events
        .iter()
        .filter(|(_, e)| matches!(e, crate::audio::Event::NoteOn { .. }))
        .map(|(at, _)| *at)
        .collect();

    // step 1 at sample 0, step 3 two beats later; the rest emits nothing
    assert_eq!(ons, vec![0, 96_000]);
}

#[test]
fn duration_sets_the_off_position() {
    let patch = seq(r#"{ { note = 69, instrument = "a", duration = 0.25 } }"#);
    let offs: Vec<u64> = patch
        .timeline
        .events
        .iter()
        .filter(|(_, e)| matches!(e, crate::audio::Event::NoteOff { .. }))
        .map(|(at, _)| *at)
        .collect();

    assert_eq!(offs, vec![12_000]); // a quarter of one beat
}

fn offs(patch: &Patch) -> Vec<u64> {
    patch
        .timeline
        .events
        .iter()
        .filter(|(_, e)| matches!(e, crate::audio::Event::NoteOff { .. }))
        .map(|(at, _)| *at)
        .collect()
}

#[test]
fn a_long_note_keeps_its_off_under_a_different_pitch() {
    // duration 3 ends at beat 3, past the note on beat 1. A different pitch
    // takes another voice, so the long note still needs its own release.
    let patch = seq(r#"{ { note = 69, instrument = "a", duration = 3 }, { note = 72 } }"#);
    assert_eq!(offs(&patch), vec![48_000 + 24_000, 3 * 48_000]);
}

#[test]
fn a_long_note_keeps_its_off_under_another_instrument() {
    let patch = seq(
        r#"{ { note = 69, instrument = "a", duration = 3 }, { note = 69, instrument = "b" } }"#,
    );
    assert_eq!(offs(&patch), vec![48_000 + 24_000, 3 * 48_000]);
}

#[test]
fn a_long_note_emits_no_off_when_its_pitch_retriggers() {
    // the retrigger takes this note's voice; left unclamped, the Off at
    // beat 3 would release the *second* note instead
    let patch = seq(r#"{ { note = 69, instrument = "a", duration = 3 }, { note = 69 } }"#);

    let offs = patch
        .timeline
        .events
        .iter()
        .filter(|(_, e)| matches!(e, crate::audio::Event::NoteOff { .. }))
        .count();
    assert_eq!(offs, 1, "only the final note should emit an off");

    let last = patch.timeline.events.last().unwrap();
    assert!(matches!(last.1, crate::audio::Event::NoteOff { .. }));
    assert_eq!(last.0, 48_000 + 24_000); // beat 1 plus half a beat
}

#[test]
fn instrument_sticks_until_a_step_changes_it() {
    let patch = seq(
        r#"{ { note = 69, instrument = "a" }, { note = 71 }, { note = 72, instrument = "b" } }"#,
    );
    let used: Vec<usize> = patch
        .timeline
        .events
        .iter()
        .filter_map(|(_, e)| match e {
            crate::audio::Event::NoteOn { instrument, .. } => Some(*instrument),
            crate::audio::Event::NoteOff { .. } => None,
        })
        .collect();

    assert_eq!(used[0], used[1], "step 2 inherits step 1's instrument");
    assert_ne!(used[1], used[2], "step 3 switches");
}

#[test]
fn an_unknown_instrument_name_is_rejected() {
    let source = r#"
        local sequence = require("synth.sequence")
        local instrument = require("synth.instrument")
        return sequence.new({
            bpm = 60,
            instruments = { a = instrument.new({}) },
            steps = { { note = 69, instrument = "nope" } },
        })
    "#;
    let error = load_source(source, "test", 48000)
        .err()
        .expect("instrument `nope` is not defined");
    assert!(
        format!("{error:#}").contains("not defined"),
        "unexpected: {error:#}"
    );
}

#[test]
fn a_step_emits_its_midi_note() {
    let patch = seq(r#"{ { note = 69, instrument = "a" } }"#);
    match patch.timeline.events[0].1 {
        crate::audio::Event::NoteOn { note, .. } => assert_eq!(note, 69),
        _ => panic!("expected a note on first"),
    }
}

fn one_instrument(attack_ms: u32, rate: u32) -> Patch {
    let source = format!(
        r#"
        local instrument = require("synth.instrument")
        local wave = require("synth.wave")
        local envelope = require("synth.envelope")
        return instrument.new({{
            waveform = wave.triangle,
            envelope = envelope.new({{
                attackMS = {attack_ms}, decayMS = 120,
                sustainLevel = 0.7, releaseMS = 250,
            }}),
        }})
        "#
    );
    load_source(&source, "test", rate).unwrap()
}

fn arranged(body: &str) -> Patch {
    let source = format!(
        r#"
        local sequence = require("synth.sequence")
        local instrument = require("synth.instrument")
        return sequence.new({{
            bpm = 60,
            instrument = "a",
            instruments = {{ a = instrument.new({{}}), b = instrument.new({{}}) }},
            {body}
        }})
        "#
    );
    load_source(&source, "test", 48000).unwrap()
}

#[test]
fn the_anchor_key_lands_on_its_note() {
    let patch = one_instrument(10, 48000);
    let notes = &patch.keymap.notes;

    assert_eq!(notes.get(&'a'), Some(&69)); // concert A
    assert_eq!(notes.get(&'s'), Some(&70)); // chromatic along the row
    assert_eq!(notes.get(&'z'), Some(&64)); // one row down is a 4th below
    assert_eq!(notes.get(&'q'), Some(&74)); // one row up is a 4th above
    assert_eq!(notes.get(&'A'), Some(&81)); // shifted is an octave up
    assert_eq!(notes.get(&'!'), Some(&91)); // and works on the number row
}

#[test]
fn changing_offset_keeps_the_anchor_on_pitch() {
    // a mandolin layout: rows a 5th apart instead of a 4th
    let source = r#"
        local instrument = require("synth.instrument")
        local wave = require("synth.wave")
        local envelope = require("synth.envelope")
        local keyboard = require("synth.keyboard")
        return instrument.new({
            waveform = wave.triangle,
            envelope = envelope.new({}),
            keyboard = keyboard.new({ offset = 7 }),
        })
    "#;
    let patch = load_source(source, "test", 48000).unwrap();

    assert_eq!(patch.keymap.notes.get(&'a'), Some(&69), "anchor holds");
    assert_eq!(patch.keymap.notes.get(&'q'), Some(&76), "a 5th above");
}

#[test]
fn an_explicit_shifted_row_of_the_wrong_length_is_rejected() {
    // derived rows cannot mismatch; an override still can, so the
    // validation stays as a guard on that one path
    let source = r#"
        local instrument = require("synth.instrument")
        local keyboard = require("synth.keyboard")
        return instrument.new({
            keyboard = keyboard.new({
                template = { rows = { "asdfghjkl" }, shifted = { "ASD" } },
            }),
        })
    "#;
    let error = load_source(source, "test", 48000)
        .err()
        .expect("row and its shifted form differ in length");
    assert!(
        format!("{error:#}").contains("differ in length"),
        "unexpected: {error:#}"
    );
}

#[test]
fn the_frequency_table_covers_every_midi_note() {
    let patch = one_instrument(10, 48000);
    assert_eq!(patch.freqs.len(), 128);
    assert!((patch.freqs[69] - 440.0).abs() < 0.01);
    assert!((patch.freqs[81] - 880.0).abs() < 0.01);
    assert!((patch.freqs[57] - 220.0).abs() < 0.01);
}

#[test]
fn list_builds_from_a_function_body() {
    // the point of seq.list: a loop inline where its result is used
    let patch = arranged(
        r#"
        patterns = {
            x = sequence.list(function(add)
                for _ = 1, 3 do
                    add({ note = 60 })
                    add({}, {})
                end
            end),
        },
        chain = sequence.list(function(add)
            for _ = 1, 2 do add("x") end
        end),
        "#,
    );

    // 3 iterations × 3 steps = 9 per pattern, chained twice
    assert_eq!(patch.steps, 18);
    assert_eq!(patch.chain, 2);
}

#[test]
fn a_chain_concatenates_patterns() {
    let patch = arranged(
        r#"
        patterns = {
            x = { { note = 60 }, { note = 61 } },
            y = { { note = 70 } },
        },
        chain = { "x", "y", "x" },
        "#,
    );
    assert_eq!(patch.steps, 5); // 2 + 1 + 2
}

#[test]
fn loop_repeats_the_whole_chain() {
    let patch = arranged(
        r#"
        patterns = { x = { { note = 60 }, {} } },
        chain = { "x" },
        loop = 3,
        "#,
    );
    assert_eq!(patch.steps, 6); // 2 steps × 1 chain entry × 3 loops
}

#[test]
fn a_reused_pattern_resolves_to_the_same_instrument_each_time() {
    // `y` sets instrument "b"; `x` must still get the sequence default both
    // times it appears, not inherit "b" from the pass before it
    let patch = arranged(
        r#"
        patterns = {
            x = { { note = 60 } },
            y = { { note = 70, instrument = "b" } },
        },
        chain = { "x", "y", "x" },
        "#,
    );
    let used: Vec<usize> = patch
        .timeline
        .events
        .iter()
        .filter_map(|(_, e)| match e {
            crate::audio::Event::NoteOn { instrument, .. } => Some(*instrument),
            crate::audio::Event::NoteOff { .. } => None,
        })
        .collect();

    assert_eq!(used.len(), 3);
    assert_eq!(used[0], used[2], "both appearances of `x` use the default");
    assert_ne!(used[0], used[1], "`y` switches away");
}

#[test]
fn a_chain_naming_an_undefined_pattern_is_rejected() {
    let source = r#"
        local sequence = require("synth.sequence")
        return sequence.new({
            patterns = { x = {} },
            chain = { "nope" },
        })
    "#;
    let error = load_source(source, "test", 48000)
        .err()
        .expect("pattern `nope` is not defined");
    assert!(
        format!("{error:#}").contains("not defined"),
        "unexpected: {error:#}"
    );
}

// The slopes are Lua, so they are exercised through a real bake. Rate
// 1000 makes milliseconds map 1:1 to samples, so `ms` is the step count.
// attack rises 0 -> 1, release falls sustainLevel -> 0.
fn baked(ctor: &str, ms: u32) -> (Vec<f32>, Vec<f32>) {
    let source = format!(
        r#"
        local slope = require("synth.slope")
        local instrument = require("synth.instrument")
        local wave = require("synth.wave")
        local envelope = require("synth.envelope")
        return instrument.new({{
            waveform = wave.triangle,
            envelope = envelope.new({{
                attackMS = {ms},
                attackFunc = {ctor},
                sustainLevel = 1,
                releaseMS = {ms},
                releaseFunc = {ctor},
            }}),
        }})
        "#
    );
    let patch = load_source(&source, "test", 1000).unwrap();
    let instrument = &patch.timeline.instruments[0];
    (instrument.attack.clone(), instrument.release.clone())
}

#[test]
fn an_exponential_slope_starts_at_initial_and_curves() {
    let (attack, _) = baked("slope.exponential(3)", 8);

    assert_eq!(attack.len(), 8);
    assert_eq!(attack[0], 0.0, "starts exactly at initial, like linear");

    // hand-checked against (1 - e^-3t) / (1 - e^-3)
    for (index, expected) in [(1, 0.329), (2, 0.555), (4, 0.818), (7, 0.976)] {
        assert!(
            (attack[index] - expected).abs() < 1e-3,
            "attack[{index}] = {} want {expected}",
            attack[index]
        );
    }

    // curved, not straight: linear puts the midpoint at 0.5
    let (linear, _) = baked("slope.linear()", 8);
    assert!(attack[4] > linear[4] + 0.2);
}

#[test]
fn an_exponential_slope_runs_in_both_directions() {
    let (_, release) = baked("slope.exponential(3)", 8);

    assert_eq!(release[0], 1.0);
    for pair in release.windows(2) {
        assert!(
            pair[1] < pair[0],
            "a falling segment must decrease: {release:?}"
        );
    }
    // the classic quick drop and long tail: past halfway in one eighth
    assert!(release[1] < 0.7);
}

#[test]
fn handing_over_a_slope_constructor_uncalled_is_caught() {
    // the mistake is the missing (), which Rust then calls with three
    // arguments; the guard names it rather than failing on a type
    let source = r#"
        local slope = require("synth.slope")
        local instrument = require("synth.instrument")
        local envelope = require("synth.envelope")
        return instrument.new({
            envelope = envelope.new({ attackMS = 10, attackFunc = slope.exponential }),
        })
    "#;
    let error = load_source(source, "test", 1000)
        .err()
        .expect("slope.exponential handed over uncalled");
    assert!(
        format!("{error:#}").contains("it is not one itself"),
        "unexpected: {error:#}"
    );
}

#[test]
fn a_zero_length_slope_is_empty() {
    assert!(baked("slope.exponential(3)", 0).0.is_empty());
    assert!(baked("slope.linear()", 0).0.is_empty());
}

#[test]
fn bakes_the_triangle_landmarks() {
    let patch = one_instrument(10, 48000);
    let wave = &patch.timeline.instruments[0].wave;
    assert_eq!(wave.len(), 2048);

    for (index, expected) in [(0, 0.0), (512, 1.0), (1024, 0.0), (1536, -1.0)] {
        assert!(
            (wave[index] - expected).abs() < 1e-4,
            "wave[{index}] = {} want {expected}",
            wave[index]
        );
    }
}

#[test]
fn envelope_lengths_follow_the_sample_rate() {
    // 10 ms of attack is a different number of samples per device
    assert_eq!(
        one_instrument(10, 48000).timeline.instruments[0]
            .attack
            .len(),
        480
    );
    assert_eq!(
        one_instrument(10, 44100).timeline.instruments[0]
            .attack
            .len(),
        441
    );
}

// the only thing worth asserting about a file the author tunes by ear is
// that it still loads without warnings
#[test]
fn the_shipped_example_loads_clean() {
    let patch = load(Path::new("lua/example.lua"), 48000).unwrap();

    assert!(patch.bpm > 0.0, "example should be a sequence");
    assert!(!patch.reports.is_empty());
    assert!(!patch.timeline.events.is_empty());
    for (name, report) in &patch.reports {
        assert!(
            report.warnings.is_empty(),
            "instrument {name}: {:?}",
            report.warnings
        );
    }
}
