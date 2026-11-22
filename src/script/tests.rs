use crate::spec::{Instrument, Key};

use super::bake::{inspect, rising_zero};
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

/// A linear stage `ms` long; at the 1000 Hz rate `inspect` is given
/// below, one sample per ms.
fn segment(ms: f64) -> crate::spec::Segment {
    crate::spec::Segment {
        curve: vec![0.0, 1.0],
        ms,
    }
}

fn instrument_with(wave: Vec<f32>) -> Instrument {
    Instrument {
        wave: wave.into(),
        attack: segment(2.0),
        decay: segment(1.0),
        sustain_level: 0.7,
        release: segment(2.0),
        voice_count: 1,
    }
}

#[test]
fn clean_triangle_raises_nothing() {
    let report = inspect(&instrument_with(wave_from(triangle, 2048)), 1000, 0, &[]);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert_eq!(report.peak, 1.0);
    assert!(report.dc.abs() < 1e-6);
}

#[test]
fn flags_clamping() {
    let report = inspect(&instrument_with(wave_from(triangle, 2048)), 1000, 17, &[]);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("17 points")));
}

#[test]
fn flags_dc_offset() {
    // a triangle lifted entirely above zero
    let report = inspect(
        &instrument_with(wave_from(|p| triangle(p) * 0.4 + 0.5, 2048)),
        1000,
        0,
        &[],
    );
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("DC offset")));
}

#[test]
fn flags_wrap_discontinuity() {
    // a saw ends at +1 and restarts at -1: a step of 2.0 every cycle
    let report = inspect(
        &instrument_with(wave_from(|p| 2.0 * p - 1.0, 2048)),
        1000,
        0,
        &[],
    );
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("wrap smoothly")));
}

#[test]
fn a_jump_the_shape_already_makes_is_not_flagged_at_the_wrap() {
    // a square jumps mid-cycle as well as at the wrap, so the wrap is no
    // rougher than the shape
    let report = inspect(
        &instrument_with(wave_from(|p| if p < 0.5 { 1.0 } else { -1.0 }, 2048)),
        1000,
        0,
        &[],
    );
    assert!(!report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("wrap smoothly")));
}

#[test]
fn flags_a_wave_that_never_rises_through_zero() {
    let report = inspect(
        &instrument_with(wave_from(|p| triangle(p) * 0.4 + 0.5, 2048)),
        1000,
        0,
        &[],
    );
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("never rises through 0")));
}

#[test]
fn a_cycle_starts_at_its_first_rise_through_zero() {
    // a saw reaches 0 exactly, halfway
    let saw = [-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75];
    assert_eq!(rising_zero(&saw), Some(4));
    // a square never touches 0, so the first positive point after the
    // crossing counts
    assert_eq!(rising_zero(&[-1.0, -1.0, 1.0, 1.0]), Some(2));
    // the point before the first is the last, since a cycle wraps
    assert_eq!(rising_zero(&[1.0, 1.0, -1.0, -1.0]), Some(0));
    assert_eq!(rising_zero(&[0.5, 0.25, 0.5, 0.75]), None);
}

#[test]
fn a_saw_bakes_starting_from_zero_rising() {
    let source = r#"
        local instrument = require("synth.instrument")
        return instrument({
            waveform = function(phase) return 2 * phase - 1 end,
        })
    "#;
    let patch = load_source(source, "test", 48000).unwrap();
    let wave = patch.timeline.instruments[0].wave.frame(0);
    assert_eq!(wave[0], 0.0);
    assert!(wave[1] > 0.0);
    assert!(wave[wave.len() - 1] < 0.0);
}

#[test]
fn flags_weak_peak() {
    let report = inspect(
        &instrument_with(wave_from(|p| triangle(p) * 0.2, 2048)),
        1000,
        0,
        &[],
    );
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("peaks at only")));
}

#[test]
fn flags_zero_release() {
    // sustain outside 0..1 is no longer a warning here: schema::ENVELOPE
    // rejects it before anything is baked
    let mut instrument = instrument_with(wave_from(triangle, 2048));
    instrument.release = segment(0.0);
    let report = inspect(&instrument, 1000, 0, &[]);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("release = 0")));
}

// 48000 Hz at 60 bpm makes one beat exactly 48000 samples
fn seq(steps: &str) -> Patch {
    let source = format!(
        r#"
        local sequence = require("synth.sequence")
        local instrument = require("synth.instrument")
        local wave = require("synth.wave")
        local envelope = require("synth.envelope")
        return sequence({{
            bpm = 60,
            instruments = {{ a = instrument({{}}), b = instrument({{}}) }},
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
        .filter(|(_, e)| matches!(e, crate::spec::Event::NoteOn { .. }))
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
        .filter(|(_, e)| matches!(e, crate::spec::Event::NoteOff { .. }))
        .map(|(at, _)| *at)
        .collect();

    assert_eq!(offs, vec![12_000]); // a quarter of one beat
}

fn offs(patch: &Patch) -> Vec<u64> {
    patch
        .timeline
        .events
        .iter()
        .filter(|(_, e)| matches!(e, crate::spec::Event::NoteOff { .. }))
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
        .filter(|(_, e)| matches!(e, crate::spec::Event::NoteOff { .. }))
        .count();
    assert_eq!(offs, 1, "only the final note should emit an off");

    let last = patch.timeline.events.last().unwrap();
    assert!(matches!(last.1, crate::spec::Event::NoteOff { .. }));
    assert_eq!(last.0, 48_000 + 24_000); // beat 1 plus half a beat
}

#[test]
fn an_off_landing_on_its_retrigger_is_dropped() {
    // duration 1 ends on the very sample the retrigger starts. The clamp
    // keeps only Offs strictly before it, so the retrigger's own Off is the
    // one that ends the voice
    let patch = seq(r#"{ { note = 69, instrument = "a", duration = 1 }, { note = 69 } }"#);
    assert_eq!(offs(&patch), vec![48_000 + 24_000]);
}

#[test]
fn an_off_landing_on_a_different_pitch_is_kept() {
    let patch = seq(r#"{ { note = 69, instrument = "a", duration = 1 }, { note = 72 } }"#);
    assert_eq!(offs(&patch), vec![48_000, 48_000 + 24_000]);
}

#[test]
fn a_rest_releases_every_note_on_its_instrument() {
    let patch =
        seq(r#"{ { note = 69, instrument = "a", duration = 3 }, { note = 72 }, { off = true } }"#);
    let events = &patch.timeline.events;
    let crate::spec::Event::NoteOn { instrument, .. } = events[0].1 else {
        panic!("first event should be a note-on");
    };
    let rests: Vec<_> = events
        .iter()
        .filter(|(_, e)| matches!(e, crate::spec::Event::AllNotesOff { .. }))
        .collect();
    assert_eq!(
        rests,
        vec![&(2 * 48_000, crate::spec::Event::AllNotesOff { instrument })]
    );
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
            crate::spec::Event::NoteOn { instrument, .. } => Some(*instrument),
            crate::spec::Event::NoteOff { .. }
            | crate::spec::Event::AllNotesOff { .. }
            | crate::spec::Event::AllSoundOff { .. } => None,
        })
        .collect();

    assert_eq!(used[0], used[1], "step 2 inherits step 1's instrument");
    assert_ne!(used[1], used[2], "step 3 switches");
}

fn with_voices(voice_count: &str) -> anyhow::Result<Patch> {
    let source = format!(
        r#"
        local sequence = require("synth.sequence")
        local instrument = require("synth.instrument")
        return sequence({{
            bpm = 60,
            instruments = {{ a = instrument({{ {voice_count} }}) }},
            steps = {{ {{ note = 69, instrument = "a" }} }},
        }})
        "#
    );
    load_source(&source, "test", 48000)
}

#[test]
fn voice_count_defaults_to_four() {
    let patch = with_voices("").unwrap();
    assert_eq!(patch.timeline.instruments[0].voice_count, 4);
}

#[test]
fn voice_count_reaches_the_instrument() {
    let patch = with_voices("voice_count = 8").unwrap();
    assert_eq!(patch.timeline.instruments[0].voice_count, 8);
}

#[test]
fn a_step_emits_its_midi_note() {
    let patch = seq(r#"{ { note = 69, instrument = "a" } }"#);
    match patch.timeline.events[0].1 {
        crate::spec::Event::NoteOn { note, .. } => assert_eq!(note, 69),
        _ => panic!("expected a note on first"),
    }
}

fn one_instrument(attack_ms: u32, rate: u32) -> Patch {
    let source = format!(
        r#"
        local instrument = require("synth.instrument")
        local wave = require("synth.wave")
        local envelope = require("synth.envelope")
        return instrument({{
            waveform = wave.triangle,
            envelope = envelope({{
                attack = {attack_ms}, decay = 120,
                sustain = 0.7, release = 250,
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
        return sequence({{
            bpm = 60,
            instrument = "a",
            instruments = {{ a = instrument({{}}), b = instrument({{}}) }},
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
        return instrument({
            waveform = wave.triangle,
            envelope = envelope({}),
            keyboard = keyboard({ layout = keyboard.grid({ offset = 7 }) }),
        })
    "#;
    let patch = load_source(source, "test", 48000).unwrap();

    assert_eq!(patch.keymap.notes.get(&'a'), Some(&69), "anchor holds");
    assert_eq!(patch.keymap.notes.get(&'q'), Some(&76), "a 5th above");
}

/// Loads an instrument whose keyboard is `keyboard`, a Lua expression.
fn with_keyboard(keyboard: &str) -> anyhow::Result<Patch> {
    let source = format!(
        r#"
        local synth = require("synth")
        return synth.instrument({{
            waveform = synth.wave.triangle,
            envelope = synth.envelope({{}}),
            keyboard = {keyboard},
        }})
        "#
    );
    load_source(&source, "test", 48000)
}

#[test]
fn a_patch_can_write_its_own_notes() {
    let patch = with_keyboard(
        r#"synth.keyboard({ layout = {
            notes = { a = 60, ["é"] = 62 }, octave_up = {}, octave_down = {}, help = {},
        } })"#,
    )
    .unwrap();

    assert_eq!(patch.keymap.notes.len(), 2);
    assert_eq!(patch.keymap.notes.get(&'a'), Some(&60));
    assert_eq!(patch.keymap.notes.get(&'é'), Some(&62));
    assert_eq!(patch.keymap.name, None, "unnamed notes read as custom");
}

#[test]
fn the_default_layout_is_named() {
    let patch = with_keyboard("synth.keyboard({})").unwrap();
    assert_eq!(patch.keymap.name.as_deref(), Some("qwerty"));
}

#[test]
fn the_grid_reads_keys_outside_ascii() {
    let patch = with_keyboard(
        r#"synth.keyboard({ layout = synth.keyboard.grid({
            note = 60,
            template = {
                anchor = "ö", rows = { "äöü" }, shifted = { "ÄÖÜ" },
                octave_up = {}, octave_down = {}, help = {},
            },
        }) })"#,
    )
    .unwrap();
    let notes = &patch.keymap.notes;

    assert_eq!(notes.get(&'ä'), Some(&59));
    assert_eq!(notes.get(&'ü'), Some(&61));
    assert_eq!(notes.get(&'Ö'), Some(&72));
}

#[test]
fn every_built_in_layout_loads_with_a4_on_its_anchor() {
    for (layout, anchor) in [
        ("qwerty", 'a'),
        ("qwertz", 'a'),
        ("azerty", 'q'),
        ("dvorak", 'a'),
        ("colemak", 'a'),
    ] {
        let patch = with_keyboard(&format!(
            "synth.keyboard({{ layout = synth.keyboard.{layout} }})"
        ))
        .unwrap_or_else(|error| panic!("{layout}: {error:#}"));
        let keymap = &patch.keymap;
        assert_eq!(keymap.name.as_deref(), Some(layout));
        assert_eq!(keymap.notes.get(&anchor), Some(&69), "{layout}");
    }
}

#[test]
fn a_position_plays_the_same_note_in_every_layout() {
    // the top row's fourth key: r on qwerty, p on dvorak, r on azerty
    for (layout, key) in [
        ("qwerty", 'r'),
        ("dvorak", 'p'),
        ("azerty", 'r'),
        ("colemak", 'p'),
    ] {
        let patch = with_keyboard(&format!(
            "synth.keyboard({{ layout = synth.keyboard.{layout} }})"
        ))
        .unwrap();
        assert_eq!(patch.keymap.notes.get(&key), Some(&77), "{layout}");
    }
}

#[test]
fn a_layout_brings_its_own_control_keys() {
    // qwerty's octave and help characters are notes on qwertz
    let patch = with_keyboard("synth.keyboard({ layout = synth.keyboard.qwertz })").unwrap();
    let keymap = &patch.keymap;

    assert!(keymap.notes.contains_key(&'='), "shift 0 plays");
    assert!(keymap.notes.contains_key(&'/'), "shift 7 plays");
    assert_eq!(keymap.octave_down, vec![Key::Char('ß'), Key::Char('?')]);
    assert_eq!(keymap.help, vec![Key::Char('-'), Key::Char('_')]);
}

#[test]
fn a_keyboard_control_replaces_the_layouts() {
    let source = r#"
        local synth = require("synth")
        local patch = synth.instrument({
            waveform = synth.wave.triangle,
            envelope = synth.envelope({}),
            keyboard = synth.keyboard({ layout = synth.keyboard.qwerty, octave_up = { "]" } }),
        })
        -- the shared layout is copied, not changed
        assert(synth.keyboard.qwerty.octave_up[1] == "=")
        return patch
    "#;
    let keymap = load_source(source, "test", 48000).unwrap().keymap;

    assert_eq!(keymap.octave_up, vec![Key::Char(']')]);
    assert_eq!(keymap.octave_down, vec![Key::Char('-'), Key::Char('_')]);
    assert_eq!(keymap.name.as_deref(), Some("qwerty"));
}

#[test]
fn the_keyboard_starts_an_octave_down_by_default() {
    let keymap = with_keyboard("synth.keyboard({})").unwrap().keymap;
    assert_eq!(
        (keymap.octave, keymap.octave_low, keymap.octave_high),
        (-1, 2, 3)
    );
}

#[test]
fn octave_limits_are_set_separately() {
    let keymap = with_keyboard("synth.keyboard({ octave = 0, octave_low = 3, octave_high = 4 })")
        .unwrap()
        .keymap;
    assert_eq!(
        (keymap.octave, keymap.octave_low, keymap.octave_high),
        (0, 3, 4)
    );
}

#[test]
fn an_octave_out_of_bounds_is_named() {
    for (field, expected) in [
        ("octave = 11", "`octave` is 11; must be -10 to 10"),
        ("octave_low = -1", "`octave_low` is -1; must be 0 to 10"),
        ("octave_high = 1.5", "`octave_high` must be a whole number"),
        ("octave_range = 4", "unknown field `octave_range`"),
    ] {
        let error = with_keyboard(&format!("synth.keyboard({{ {field} }})"))
            .err()
            .expect(field);
        assert!(
            format!("{error:#}").contains(expected),
            "{field}: {error:#}"
        );
    }
}

#[test]
fn a_note_outside_midi_is_named() {
    for (note, shown) in [("128", "128"), ("-1", "-1"), ("60.5", "60.5")] {
        let error = with_keyboard(&format!(
            "synth.keyboard({{ layout = {{
                notes = {{ a = {note} }}, octave_up = {{}}, octave_down = {{}}, help = {{}},
            }} }})"
        ))
        .err()
        .expect("not a MIDI note");
        assert!(
            format!("{error:#}").contains(&format!("keyboard.layout.notes['a'] is {shown}")),
            "unexpected: {error:#}"
        );
    }
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
fn a_patch_tuning_replaces_equal_temperament() {
    let source = r#"
        local synth = require("synth")
        return synth.instrument({
            tuning = synth.tuning.equal({ reference_hz = 432, tones = 19 }),
        })
    "#;
    let patch = load_source(source, "test", 48000).unwrap();
    assert!((patch.freqs[69] - 432.0).abs() < 0.01);
    // 19 steps up is one octave in 19-EDO
    assert!((patch.freqs[88] - 864.0).abs() < 0.01);
}

#[test]
fn constructors_leave_the_callers_table_alone() {
    // a patch reusing one options table must not find defaults written into it
    let source = r#"
        local synth = require("synth")
        local opts = { attack = 5 }
        local first = synth.envelope(opts)
        assert(opts.release == nil, "defaults leaked into the caller's table")
        assert(first ~= opts, "returned the caller's table")
        return synth.instrument({ envelope = first })
    "#;
    load_source(source, "test", 48000).unwrap();
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
            crate::spec::Event::NoteOn { instrument, .. } => Some(*instrument),
            crate::spec::Event::NoteOff { .. }
            | crate::spec::Event::AllNotesOff { .. }
            | crate::spec::Event::AllSoundOff { .. } => None,
        })
        .collect();

    assert_eq!(used.len(), 3);
    assert_eq!(used[0], used[2], "both appearances of `x` use the default");
    assert_ne!(used[0], used[1], "`y` switches away");
}

// A stage's levels, sample by sample, as render reads them: stretched to
// its length at `rate` and scaled from `from` to `to`.
fn levels(stage: &crate::spec::Segment, rate: f64, from: f32, to: f32) -> Vec<f32> {
    let len = stage.samples(rate);
    (0..len)
        .map(|i| from + (to - from) * stage.along(i, len))
        .collect()
}

// The slopes are Lua, so they are exercised through a real bake. Rate
// 1000 makes milliseconds map 1:1 to samples, so `ms` is the step count.
// attack rises 0 -> 1, release falls sustain -> 0.
fn baked(ctor: &str, ms: u32) -> (Vec<f32>, Vec<f32>) {
    let source = format!(
        r#"
        local slope = require("synth.slope")
        local instrument = require("synth.instrument")
        local wave = require("synth.wave")
        local envelope = require("synth.envelope")
        return instrument({{
            waveform = wave.triangle,
            envelope = envelope({{
                attack = {ms},
                attack_curve = {ctor},
                sustain = 1,
                release = {ms},
                release_curve = {ctor},
            }}),
        }})
        "#
    );
    let patch = load_source(&source, "test", 1000).unwrap();
    let instrument = &patch.timeline.instruments[0];
    (
        levels(&instrument.attack, 1000.0, 0.0, 1.0),
        levels(&instrument.release, 1000.0, 1.0, 0.0),
    )
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
fn any_function_of_t_is_a_curve() {
    let (attack, release) = baked("function(t) return t * t end", 4);
    assert_eq!(attack, [0.0, 0.0625, 0.25, 0.5625]);
    // the same shape, scaled to fall from sustain (1) to 0
    assert_eq!(release, [1.0, 0.9375, 0.75, 0.4375]);
}

#[test]
fn a_curve_outside_0_to_1_is_clamped_and_named() {
    let source = r#"
        local synth = require("synth")
        return synth.instrument({
            envelope = synth.envelope({
                attack = 10,
                attack_curve = function() return 1000 end,
            }),
        })
    "#;
    let patch = load_source(source, "test", 1000).unwrap();
    let curve = &patch.timeline.instruments[0].attack.curve;
    assert!(curve.iter().all(|&along| along == 1.0), "{curve:?}");
    let report = &patch.reports[0].1;
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.message == "envelope.attack_curve left 0..1 at 1025 points and was clamped"),
        "{:?}",
        report.diagnostics
    );
    assert!(!patch.clean());
}

#[test]
fn a_zero_length_slope_is_empty() {
    assert!(baked("slope.exponential(3)", 0).0.is_empty());
    assert!(baked("slope.linear()", 0).0.is_empty());
}

#[test]
fn bakes_the_triangle_landmarks() {
    let patch = one_instrument(10, 48000);
    let table = &patch.timeline.instruments[0].wave;
    // one waveform is a table of one frame, until wavetables add more
    assert_eq!(table.frame_count(), 1);
    let wave = table.frame(0);
    assert_eq!(wave.len(), 2048);

    for (index, expected) in [(0, 0.0), (512, 1.0), (1024, 0.0), (1536, -1.0)] {
        assert!(
            (wave[index] - expected).abs() < 1e-4,
            "wave[{index}] = {} want {expected}",
            wave[index]
        );
    }
}

// stretching reads between baked points, so a long stage at a high rate
// must still match the curve itself, sample by sample
#[test]
fn a_stretched_curve_matches_the_curve() {
    let source = r#"
        local synth = require("synth")
        return synth.instrument({
            envelope = synth.envelope({
                attack = 2000,
                attack_curve = synth.slope.exponential(3),
            }),
        })
    "#;
    let rate = 96000.0;
    let patch = load_source(source, "test", rate as u32).unwrap();
    let attack = levels(&patch.timeline.instruments[0].attack, rate, 0.0, 1.0);
    assert_eq!(attack.len(), 192_000);
    let exact = |t: f64| (1.0 - (-3.0 * t).exp()) / (1.0 - (-3.0f64).exp());
    for (i, &level) in attack.iter().enumerate() {
        let want = exact(i as f64 / attack.len() as f64);
        assert!(
            (f64::from(level) - want).abs() < 1e-5,
            "attack[{i}] = {level} want {want}"
        );
    }
}

#[test]
fn a_curve_is_baked_once_whatever_its_length_or_rate() {
    let short = one_instrument(10, 48000);
    let long = one_instrument(2000, 96000);
    let (short, long) = (
        &short.timeline.instruments[0].attack,
        &long.timeline.instruments[0].attack,
    );
    assert_eq!(short.curve.len(), 1025);
    assert_eq!(short.curve, long.curve);

    // the length is stretched to at playback
    assert_eq!(short.samples(48000.0), 480);
    assert_eq!(short.samples(44100.0), 441);
    assert_eq!(long.samples(96000.0), 192_000);
}

// the only thing worth asserting about a file the author tunes by ear is
// that it still loads without warnings
#[test]
fn the_shipped_example_loads_clean() {
    let patch = load(Path::new("lua/example.lua"), 48000, None).unwrap();

    assert!(patch.bpm > 0.0, "example should be a sequence");
    assert!(!patch.reports.is_empty());
    assert!(!patch.timeline.events.is_empty());
    for (name, report) in &patch.reports {
        assert!(
            report.diagnostics.is_empty(),
            "instrument {name}: {:?}",
            report.diagnostics
        );
    }
}

/// A bare instrument whose tuning is `tuning`, a Lua function expression.
fn with_tuning(tuning: &str, rate: u32) -> anyhow::Result<Patch> {
    let source = format!(
        r#"
        local synth = require("synth")
        return synth.instrument({{ tuning = {tuning} }})
        "#
    );
    load_source(&source, "test", rate)
}

#[test]
fn equal_temperament_is_clean_at_48k() {
    let patch = with_tuning("synth.tuning.equal({})", 48000).unwrap();
    assert!(
        patch.tuning_diagnostics.is_empty(),
        "{:?}",
        patch.tuning_diagnostics
    );
    assert!(patch.clean());
}

#[test]
fn a_tuning_that_is_not_a_positive_finite_hz_is_named() {
    for (body, shown) in [
        ("math.huge", "inf"),
        ("0/0", "NaN"),
        ("0", "0"),
        ("-440", "-440"),
        // finite as a Lua number, but too large for the f32 table
        ("1e300", "1e300"),
    ] {
        let tuning = format!("function(note) if note == 69 then return {body} end return 440 end");
        let error = with_tuning(&tuning, 48000).err().expect(body);
        let message = format!("{error:#}");
        let expected =
            format!("tuning returned {shown} Hz for note 69; must be positive and finite");
        assert!(message.contains(&expected), "{body}: {message}");
    }
}

#[test]
fn notes_above_nyquist_warn_with_their_range() {
    // equal temperament reaches 12.5 kHz at note 127, over 8 kHz's Nyquist
    let patch = with_tuning("synth.tuning.equal({})", 16000).unwrap();
    assert!(!patch.clean());
    let message = patch.tuning_diagnostics[0].to_string();
    assert!(
        message.contains("notes from 120 to 127 are above Nyquist (8000 Hz at 16000 Hz)"),
        "{message}"
    );
}

#[test]
fn a_single_note_above_nyquist_is_named_alone() {
    let patch = with_tuning(
        "function(note) if note == 0 then return 30000 end return 440 end",
        48000,
    )
    .unwrap();
    let message = patch.tuning_diagnostics[0].to_string();
    assert!(
        message.contains("tuning: note 0 is above Nyquist"),
        "{message}"
    );
}

// a bare instrument file with `filter = <expr>`, or no filter for ""
fn with_filter(filter: &str) -> anyhow::Result<Patch> {
    let field = if filter.is_empty() {
        String::new()
    } else {
        format!("filter = {filter},")
    };
    let source = format!(
        r#"
        local instrument = require("synth.instrument")
        local filter = require("synth.filter")
        return instrument({{ {field} }})
        "#
    );
    load_source(&source, "test", 48000)
}

#[test]
fn no_filter_means_none() {
    assert_eq!(with_filter("").unwrap().filter, None);
}

#[test]
fn lowpass_reads_into_filter() {
    let patch = with_filter("filter.lowpass({ cutoff = 800 })").unwrap();
    assert_eq!(
        patch.filter,
        Some(crate::spec::Filter {
            kind: crate::spec::Kind::Lowpass,
            cutoff: 800.0,
            damping: std::f32::consts::SQRT_2,
            root: 1.0,
        })
    );
}

#[test]
fn bell_gain_converts_from_db() {
    let patch = with_filter("filter.bell({ cutoff = 1000, gain = 6 })").unwrap();
    let root = patch.filter.unwrap().root;
    assert!((root - 10f32.powf(6.0 / 80.0)).abs() < 1e-4, "root {root}");
}

#[test]
fn full_resonance_reaches_min_damping() {
    let patch = with_filter("filter.lowpass({ cutoff = 800, resonance = 1 })").unwrap();
    assert_eq!(patch.filter.unwrap().damping, crate::spec::MIN_DAMPING);
}

#[test]
fn filter_in_sequence_file() {
    let source = r#"
        local sequence = require("synth.sequence")
        local instrument = require("synth.instrument")
        local filter = require("synth.filter")
        return sequence({
            bpm = 60,
            instruments = { a = instrument({}) },
            steps = { { note = 69, instrument = "a" } },
            filter = filter.highpass({ cutoff = 200 }),
        })
    "#;
    let patch = load_source(source, "test", 48000).unwrap();
    assert_eq!(patch.filter.unwrap().kind, crate::spec::Kind::Highpass);
}

#[test]
fn filter_report_keeps_written_values() {
    let patch = with_filter("filter.bell({ cutoff = 1000, resonance = 0.5, gain = 6 })").unwrap();
    let report = patch.filter_report.as_ref().unwrap();
    assert_eq!(report.kind, "bell");
    assert_eq!(report.cutoff, 1000.0);
    assert_eq!(report.resonance, Some(0.5));
    assert_eq!(report.gain, Some(6.0));
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert!(patch.clean());
}

#[test]
fn shelf_report_has_no_resonance() {
    let patch = with_filter("filter.low_shelf({ cutoff = 200, gain = -3 })").unwrap();
    let report = patch.filter_report.unwrap();
    assert_eq!(report.resonance, None);
    assert_eq!(report.gain, Some(-3.0));
}

#[test]
fn cutoff_above_ceiling_warns() {
    // the ceiling at 48 kHz is 0.49 × 48000 = 23520 Hz
    let patch = with_filter("filter.lowpass({ cutoff = 30000 })").unwrap();
    let warnings = &patch.filter_report.as_ref().unwrap().diagnostics;
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].message.contains("23520"), "{}", warnings[0]);
    assert!(!patch.clean());
}

#[test]
fn cutoff_below_floor_warns() {
    let patch = with_filter("filter.highpass({ cutoff = 5 })").unwrap();
    let warnings = &patch.filter_report.as_ref().unwrap().diagnostics;
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(!patch.clean());
}

#[test]
fn the_root_reexports_every_module() {
    let lua = mlua::Lua::new();
    lua_register(&lua, Vec::new(), None).unwrap();

    // the constructors' helper is theirs, not a patch's
    for (name, _) in MODULES
        .iter()
        .filter(|(name, _)| *name != module::CONSTRUCT)
    {
        let Some(short) = name.strip_prefix("synth.") else {
            continue;
        };
        // the same table, not a copy: a module is loaded once per state
        let same: bool = lua
            .load(format!(
                r#"return rawequal(require("synth").{short}, require("{name}"))"#
            ))
            .eval()
            .unwrap();
        assert!(same, "synth.{short} is not re-exported by the root");
    }
}

#[test]
fn structure_records_every_constructed_table_by_path() {
    let source = r#"
        local synth = require("synth")
        local tri = synth.instrument({ envelope = synth.envelope({}) })
        return synth.sequence({ instruments = { tri = tri }, instrument = "tri", steps = { { note = 60 } } })
    "#;
    let patch = load_source(source, "song.lua", 48000).unwrap();
    let found: Vec<(&str, &str, &str)> = patch
        .structure
        .iter()
        .map(|node| (node.path.as_str(), node.kind.as_str(), node.at.as_str()))
        .collect();
    assert_eq!(
        found,
        vec![
            // the root is a tail call, so it is named by its file
            ("", "sequence", "song.lua"),
            ("instruments.tri", "instrument", "song.lua:3"),
            ("instruments.tri.envelope", "envelope", "song.lua:3"),
        ]
    );
}

#[test]
fn the_default_patch_loads_clean() {
    // what `synth tui` plays with no file; nothing else exercises it
    let patch = load_default(48000, None).unwrap();
    assert!(patch.clean(), "default.lua raised warnings");
    assert_eq!(patch.structure[0].kind, "instrument");
}

#[test]
fn diagnostics_name_the_line_that_built_the_value() {
    let source = r#"
        local synth = require("synth")
        local quiet = function(phase) return 0.2 * math.sin(phase * 2 * math.pi) end
        return synth.sequence({
            instruments = { soft = synth.instrument({ waveform = quiet }) },
            instrument = "soft",
            steps = { { note = 60 } },
            filter = synth.filter.lowpass({ cutoff = 5 }),
        })
    "#;
    let patch = load_source(source, "song.lua", 48000).unwrap();
    let (_, report) = &patch.reports[0];
    let peak = report
        .diagnostics
        .iter()
        .find(|d| d.message.contains("peaks at only"))
        .expect("a quiet waveform warns");
    assert_eq!(peak.at.as_deref(), Some("song.lua:5"));
    let cutoff = &patch.filter_report.as_ref().unwrap().diagnostics[0];
    assert_eq!(cutoff.at.as_deref(), Some("song.lua:8"));
}

/// Calls a constructor module, which is a table with `__call`, as Lua would.
fn construct(lua: &mlua::Lua, module: &str, opts: mlua::Table) -> mlua::Table {
    let require: mlua::Function = lua.globals().get("require").unwrap();
    let table: mlua::Table = require.call(module).unwrap();
    let call: mlua::Function = table.metatable().unwrap().get("__call").unwrap();
    call.call((table, opts)).unwrap()
}

#[test]
fn every_lua_default_passes_rust_validation() {
    // Lua owns the defaults and Rust owns what is valid; this keeps the two
    // from drifting apart
    let lua = mlua::Lua::new();
    lua_register(&lua, Vec::new(), None).unwrap();
    let empty = || lua.create_table().unwrap();

    let envelope = construct(&lua, "synth.envelope", empty());
    super::schema::ENVELOPE.check(&envelope, None).unwrap();
    let instrument = construct(&lua, "synth.instrument", empty());
    super::schema::INSTRUMENT.check(&instrument, None).unwrap();
    let keyboard = construct(&lua, "synth.keyboard", empty());
    super::schema::KEYBOARD.check(&keyboard, None).unwrap();

    // a sequence needs one step to resolve, which takes every step default
    let opts: mlua::Table = lua
        .load(r#"return { instruments = {}, steps = { {} } }"#)
        .eval()
        .unwrap();
    let sequence = construct(&lua, "synth.sequence", opts);
    super::schema::SEQUENCE.check(&sequence, None).unwrap();
    let steps: mlua::Table = sequence.get("steps").unwrap();
    for step in steps.sequence_values::<mlua::Table>() {
        super::schema::STEP.check(&step.unwrap(), None).unwrap();
    }
}

/// A patch in `tests/lua/loader/{case}/`, the folder its modules sit in.
fn fixture(case: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/lua/loader")
        .join(case)
        .join("song.lua")
}

#[test]
fn a_patch_requires_modules_beside_it() {
    load(&fixture("beside"), 48000, None).unwrap();
}

#[test]
fn a_patch_requires_from_the_patches_folder_after_its_own() {
    let patches = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lua/loader/patches");
    load(&fixture("from_patches"), 48000, Some(&patches)).unwrap();
}

#[test]
fn a_file_named_like_a_built_in_does_not_replace_it() {
    load(&fixture("shadow"), 48000, None).unwrap();
}

#[test]
fn a_missing_module_lists_only_the_patch_folder() {
    let song = fixture("missing");
    let error = load(&song, 48000, None).err().expect("nowhere.lua");
    let message = format!("{error:#}");
    let looked = song.with_file_name("nowhere.lua");
    assert!(
        message.contains(&format!("no file '{}'", looked.display())),
        "{message}"
    );
    // neither the cwd nor the system's Lua folders are searched
    assert!(!message.contains("'./nowhere.lua'"), "{message}");
    assert!(!message.contains("/usr/"), "{message}");
}

#[test]
fn a_module_name_cannot_leave_its_folder() {
    for name in ["..secret", "a..b", "a/b", ""] {
        let error = load_source(&format!("return require('{name}')"), "test", 48000)
            .err()
            .expect(name);
        assert!(
            format!("{error:#}").contains("is not a module name"),
            "{name}: {error:#}"
        );
    }
}

/// Every patch in `tests/lua/reject/` fails to load with the error on its
/// first line, `-- expect: …`, where `{file}` stands for the file's name.
/// Each file is loaded under its bare name, so an expected error never
/// holds a path that differs between machines.
#[test]
fn every_rejected_patch_fails_with_its_expected_error() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lua/reject");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "lua"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no cases in {}", dir.display());

    let mut failures = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(path).unwrap();
        let Some(expected) = source
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("-- expect: "))
        else {
            failures.push(format!("{name}: the first line must be `-- expect: …`"));
            continue;
        };
        let expected = expected.replace("{file}", &name);
        match load_source(&source, &name, 48000) {
            Ok(_) => failures.push(format!("{name}: loaded, expected `{expected}`")),
            Err(error) => {
                let message = format!("{error:#}");
                if !message.contains(&expected) {
                    failures.push(format!("{name}: expected `{expected}`\n    got {message}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn an_edited_built_in_copy_loads_in_place_of_the_built_in() {
    let patches = std::env::temp_dir().join(format!("synth-edited-load-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&patches);
    builtins::write(&patches).unwrap();
    let wave = patches.join("synth/wave.lua");
    let source = std::fs::read_to_string(&wave).unwrap();
    std::fs::write(
        &wave,
        source.replace("return wave", "wave.edited = true\nreturn wave"),
    )
    .unwrap();

    let patch = r#"
        local synth = require("synth")
        assert(require("synth.wave").edited, "the edited copy")
        return synth.instrument({})
    "#;
    load_in(patch, "test", 48000, None, Some(&patches)).unwrap();
    // without the patches folder, the built-in loads and has no mark
    assert!(load_in(patch, "test", 48000, None, None).is_err());
}

#[test]
fn every_starter_loads_in_one_patch() {
    let patches = std::env::temp_dir().join(format!("synth-starters-load-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&patches);
    starters::write(&patches).unwrap();
    let patch = load(&fixture("starters"), 48000, Some(&patches)).unwrap();
    assert!(patch.clean(), "{:?}", patch.reports);
}

#[test]
fn constructed_values_share_no_default() {
    let source = r#"
        local synth = require("synth")
        local first = synth.keyboard({})
        first.layout.notes.a = 0
        first.stop[1] = "q"
        assert(synth.keyboard({}).layout.notes.a ~= 0, "layout shared")
        assert(synth.keyboard({}).stop[1] == "escape", "stop shared")
        assert(synth.keyboard.qwerty.notes.a ~= 0, "template edited")

        local bass = synth.instrument({})
        bass.envelope.attack = 999
        assert(synth.instrument({}).envelope.attack ~= 999, "envelope shared")
        assert(getmetatable(bass.envelope).kind == "envelope", "stamp lost")
        return synth.instrument({})
    "#;
    load_source(source, "test", 48000).unwrap();
}

/// The typed reason a patch failed, found through any context added on the
/// way out.
fn rejected(source: &str) -> anyhow::Error {
    load_source(source, "patch.lua", 48000)
        .err()
        .expect("the patch should not load")
}

#[test]
fn a_step_out_of_range_is_matched_by_variant() {
    let error = rejected(
        r#"
        local synth = require("synth")
        return synth.sequence({
            instruments = { a = synth.instrument({}) },
            instrument = "a",
            steps = { { note = 60 }, { note = 200 } },
        })
        "#,
    );
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::OutOfRange { what, field: "note", value, .. })
                if what == "step 2" && *value == 200.0
        ),
        "{error:#}"
    );
}

#[test]
fn a_keyboard_out_of_range_is_matched_by_variant() {
    let error = rejected(
        r#"
        local synth = require("synth")
        return synth.instrument({ keyboard = synth.keyboard({ velocity = 0 }) })
        "#,
    );
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::OutOfRange { field: "velocity", value, .. }) if *value == 0.0
        ),
        "{error:#}"
    );
}

#[test]
fn a_cycle_is_matched_by_variant() {
    let error = rejected(
        r#"
        local synth = require("synth")
        local loop = {}
        loop.self = loop
        return synth.instrument({ extra = loop })
        "#,
    );
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::Cycle { what }) if what == "extra.self"
        ),
        "{error:#}"
    );
}

#[test]
fn a_field_of_the_wrong_kind_is_matched_by_variant() {
    let error = rejected(
        r#"
        local synth = require("synth")
        return synth.instrument({ envelope = synth.keyboard({}) })
        "#,
    );
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::WrongKind { what, expected: "envelope", got, .. })
                if what == "envelope" && got == "keyboard"
        ),
        "{error:#}"
    );
}

#[test]
fn a_key_bound_to_a_note_and_a_control_is_matched_by_variant() {
    let error = with_keyboard(
        r#"synth.keyboard({
            layout = { notes = { ["="] = 60 }, octave_up = { "=" }, octave_down = {}, help = {} },
        })"#,
    )
    .err()
    .expect("the patch should not load");
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::KeyConflict {
                what: "layout.octave_up",
                key: '=',
                note: 60
            })
        ),
        "{error:#}"
    );
}

#[test]
fn a_note_outside_midi_is_matched_by_variant() {
    let error = with_keyboard(
        r#"synth.keyboard({
            layout = { notes = { a = 128 }, octave_up = {}, octave_down = {}, help = {} },
        })"#,
    )
    .err()
    .expect("the patch should not load");
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::NoteRange { key, value }) if key == "a" && *value == 128.0
        ),
        "{error:#}"
    );
}

#[test]
fn a_tuning_that_is_not_a_positive_hz_is_matched_by_variant() {
    let error = with_tuning(
        "function(note) if note == 69 then return -440 end return 440 end",
        48000,
    )
    .err()
    .expect("the patch should not load");
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::TuningHz { note: 69, hz }) if *hz == -440.0
        ),
        "{error:#}"
    );
}

#[test]
fn a_tuning_that_returns_no_number_is_matched_by_variant() {
    let error = with_tuning("function(note) return 'a' end", 48000)
        .err()
        .expect("the patch should not load");
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::TuningCall { note: 0, .. })
        ),
        "{error:#}"
    );
}

#[test]
fn an_uncalled_slope_is_matched_by_variant() {
    let error = rejected(
        r#"
        local synth = require("synth")
        return synth.instrument({
            envelope = synth.envelope({ attack_curve = synth.slope.exponential }),
        })
        "#,
    );
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::CurveUncalled {
                field: "attack_curve"
            })
        ),
        "{error:#}"
    );
}

#[test]
fn a_curve_that_is_not_finite_is_matched_by_variant_inside_a_sequence() {
    let error = rejected(
        r#"
        local synth = require("synth")
        local nan = synth.envelope({ decay_curve = function(t) return t / t end })
        return synth.sequence({
            instruments = { a = synth.instrument({ envelope = nan }) },
            instrument = "a",
            steps = { { note = 60 } },
        })
        "#,
    );
    assert!(
        matches!(
            error.downcast_ref::<Error>(),
            Some(Error::CurveNotFinite { field: "decay_curve", t, .. }) if *t == 0.0
        ),
        "{error:#}"
    );
}
