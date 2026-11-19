use crate::spec::{Instrument, Key};

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
        voice_count: 1,
    }
}

#[test]
fn clean_triangle_raises_nothing() {
    let report = inspect(&instrument_with(wave_from(triangle, 2048)), 0);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert_eq!(report.peak, 1.0);
    assert!(report.dc.abs() < 1e-6);
}

#[test]
fn flags_clamping() {
    let report = inspect(&instrument_with(wave_from(triangle, 2048)), 17);
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
        0,
    );
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("DC offset")));
}

#[test]
fn flags_wrap_discontinuity() {
    // a saw ends at +1 and restarts at -1: a step of 2.0 every cycle
    let report = inspect(&instrument_with(wave_from(|p| 2.0 * p - 1.0, 2048)), 0);
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.message.contains("wrap smoothly")));
}

#[test]
fn flags_weak_peak() {
    let report = inspect(&instrument_with(wave_from(|p| triangle(p) * 0.2, 2048)), 0);
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
    instrument.release = vec![];
    let report = inspect(&instrument, 0);
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
            crate::spec::Event::NoteOff { .. } | crate::spec::Event::AllNotesOff { .. } => None,
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
        return sequence({
            bpm = 60,
            instruments = { a = instrument({}) },
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
fn voice_count_defaults_to_mono() {
    let patch = with_voices("").unwrap();
    assert_eq!(patch.timeline.instruments[0].voice_count, 1);
}

#[test]
fn voice_count_reaches_the_instrument() {
    let patch = with_voices("voice_count = 8").unwrap();
    assert_eq!(patch.timeline.instruments[0].voice_count, 8);
}

#[test]
fn zero_voices_is_rejected() {
    let error = with_voices("voice_count = 0").err().expect("0 voices");
    assert!(
        format!("{error:#}").contains("`voice_count` is 0; must be at least 1"),
        "unexpected: {error:#}"
    );
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

#[test]
fn an_explicit_shifted_row_of_the_wrong_length_is_rejected() {
    // derived rows cannot mismatch; an override still can, so the
    // validation stays as a guard on that one path
    let source = r#"
        local instrument = require("synth.instrument")
        local keyboard = require("synth.keyboard")
        return instrument({
            keyboard = keyboard({
                layout = keyboard.grid({
                    template = { anchor = "a", rows = { "asdfghjkl" }, shifted = { "ASD" } },
                }),
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
fn a_control_override_on_a_note_key_is_named() {
    let error = with_keyboard(r#"synth.keyboard({ help = { "q" } })"#)
        .err()
        .expect("q plays a note on qwerty");
    assert!(
        format!("{error:#}").contains("on 'q', which keyboard.layout.help also binds"),
        "unexpected: {error:#}"
    );
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
fn a_key_that_is_both_a_note_and_a_control_is_named() {
    let error = with_keyboard(
        r#"synth.keyboard({ layout = {
            notes = { ["="] = 60 }, octave_up = { "=" }, octave_down = {}, help = {},
        } })"#,
    )
    .err()
    .expect("= is octave_up");
    assert!(
        format!("{error:#}").contains(
            "keyboard.layout plays note 60 on '=', which keyboard.layout.octave_up also binds"
        ),
        "unexpected: {error:#}"
    );
}

#[test]
fn a_notes_key_longer_than_one_character_is_named() {
    let error = with_keyboard(
        r"synth.keyboard({ layout = {
            notes = { ab = 60 }, octave_up = {}, octave_down = {}, help = {},
        } })",
    )
    .err()
    .expect("a two-character key cannot be pressed");
    assert!(
        format!("{error:#}").contains("key 'ab' must be one character"),
        "unexpected: {error:#}"
    );
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
fn the_grid_names_an_unknown_field() {
    let error = with_keyboard("synth.keyboard({ layout = synth.keyboard.grid({ offest = 7 }) })")
        .err()
        .expect("offest is misspelled");
    assert!(
        format!("{error:#}").contains("keyboard.grid: unknown field 'offest'"),
        "unexpected: {error:#}"
    );
}

#[test]
fn grid_fields_on_the_keyboard_are_named() {
    // the keyboard used to take these directly; they now belong to the grid
    let error = with_keyboard("synth.keyboard({ offset = 7 })")
        .err()
        .expect("offset is a grid field");
    assert!(
        format!("{error:#}").contains("unknown field `offset`"),
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
fn the_old_new_entry_points_are_gone() {
    let source = r#"
        local synth = require("synth")
        return synth.instrument.new({})
    "#;
    assert!(load_source(source, "test", 48000).is_err());
}

#[test]
fn tuning_equal_rejects_unknown_fields() {
    let source = r#"
        local synth = require("synth")
        return synth.instrument({ tuning = synth.tuning.equal({ A4 = 440 }) })
    "#;
    let error = load_source(source, "test", 48000)
        .err()
        .expect("A4 is the old name");
    assert!(
        format!("{error:#}").contains("unknown field 'A4'"),
        "unexpected: {error:#}"
    );
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
            crate::spec::Event::NoteOff { .. } | crate::spec::Event::AllNotesOff { .. } => None,
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
        return sequence({
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
        return instrument({
            envelope = envelope({ attack = 10, attack_curve = slope.exponential }),
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

#[track_caller]
fn rejects(filter: &str, needle: &str) {
    let error = with_filter(filter).err().expect("should be rejected");
    assert!(
        format!("{error:#}").contains(needle),
        "expected `{needle}` in: {error:#}"
    );
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
fn misspelled_filter_field_is_named() {
    rejects("filter.lowpass({ cutoff = 800, cutof = 900 })", "cutof");
}

#[test]
fn gain_on_lowpass_is_rejected() {
    rejects("filter.lowpass({ cutoff = 800, gain = 3 })", "gain");
}

#[test]
fn resonance_out_of_range_is_rejected() {
    rejects(
        "filter.lowpass({ cutoff = 800, resonance = 1.5 })",
        "resonance",
    );
}

#[test]
fn cutoff_must_be_positive() {
    rejects("filter.lowpass({ cutoff = 0 })", "cutoff");
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
    lua_register(&lua).unwrap();

    for (name, _) in MODULES {
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

// expects `load_source` to fail with a message containing `needle`
fn rejects_patch(source: &str, needle: &str) {
    let error = load_source(source, "test", 48000)
        .err()
        .unwrap_or_else(|| panic!("expected an error containing {needle:?}"));
    assert!(
        format!("{error:#}").contains(needle),
        "unexpected: {error:#}"
    );
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
fn a_plain_root_is_rejected() {
    rejects_patch("return {}", "a patch must return synth.instrument");
}

#[test]
fn a_plain_table_where_a_kind_belongs_names_the_constructor() {
    rejects_patch(
        r#"
        local synth = require("synth")
        return synth.instrument({ envelope = { attack = 5 } })
        "#,
        "envelope: expected an envelope, got a plain table; build it with synth.envelope({…})",
    );
}

#[test]
fn the_wrong_kind_names_where_it_was_made() {
    rejects_patch(
        r#"
        local synth = require("synth")
        local keys = synth.keyboard({})
        return synth.instrument({ envelope = keys })
        "#,
        "envelope: expected an envelope, got a keyboard made at test:3",
    );
}

#[test]
fn a_cycle_is_rejected() {
    rejects_patch(
        r#"
        local synth = require("synth")
        local loop = {}
        loop.self = loop
        return synth.instrument({ extra = loop })
        "#,
        "contains itself",
    );
}

#[test]
fn the_default_patch_loads_clean() {
    // what `synth tui` plays with no file; nothing else exercises it
    let patch = load_default(48000).unwrap();
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

#[test]
fn out_of_range_values_name_the_field_and_the_line() {
    rejects_patch(
        r#"
        local synth = require("synth")
        return synth.instrument({
            envelope = synth.envelope({ sustain = 1.5 }),
        })
        "#,
        "envelope made at test:4: `sustain` is 1.5; must be 0 to 1",
    );
    rejects_patch(
        r#"
        local synth = require("synth")
        return synth.instrument({ keyboard = synth.keyboard({ velocity = 0 }) })
        "#,
        "keyboard made at test:3: `velocity` is 0; must be 1 to 127",
    );
}

#[test]
fn a_bad_step_is_named_by_its_position() {
    rejects_patch(
        r#"
        local synth = require("synth")
        return synth.sequence({
            instruments = { a = synth.instrument({}) },
            instrument = "a",
            steps = { { note = 60 }, { note = 200 } },
        })
        "#,
        "step 2: `note` is 200; must be 0 to 127",
    );
}

#[test]
fn a_fractional_count_is_rejected() {
    rejects_patch(
        r#"
        local synth = require("synth")
        return synth.instrument({ voice_count = 2.5 })
        "#,
        "`voice_count` must be a whole number, got 2.5",
    );
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
    lua_register(&lua).unwrap();
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
