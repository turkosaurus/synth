use anyhow::Context;
use std::time::Duration;
use tracing::{debug_span, instrument, warn};

use crate::audio::Voice;

const LUA_OSC: &str = include_str!("../lua/osc.lua");
const LUA_VOICE: &str = include_str!("../lua/voice.lua");
const LUA_SEQUENCE: &str = include_str!("../lua/sequence.lua");

// points per waveform cycle: enough that the lowest note still reads
// most of them, small enough that 8 KB stays in L1
const TABLE_SIZE: usize = 2048;


// require checks package.preload before any filesystem search, so the
// embedded libraries resolve with no cwd dependency
fn lua_register(lua: &mlua::Lua) -> anyhow::Result<()> {
    let package: mlua::Table = lua.globals().get("package")?;
    let preload: mlua::Table = package.get("preload")?;

    for (name, src) in [
        ("synth.osc", LUA_OSC),
        ("synth.voice", LUA_VOICE),
        ("synth.sequence", LUA_SEQUENCE),
    ] {
        let loader = lua.create_function(move |lua, _: mlua::Value| {
            lua.load(src).set_name(name).eval::<mlua::Value>()
        })?;
        preload.set(name, loader)?;
    }
    Ok(())
}

pub fn check(path: &std::path::Path, sample_rate: u32) -> anyhow::Result<bool> {
    let patch = load(path, sample_rate)?;
    let ms = |samples: usize| format!("{:.1} ms", samples as f64 * 1000.0 / sample_rate as f64);
    let mut clean = true;

    if patch.bpm > 0.0 {
        let mut rows = vec![
            ("bpm", format!("{:.0}", patch.bpm)),
            ("division", format!("{:.0}", patch.division)),
            ("step", format!("{:.1} ms", patch.step_ms)),
        ];
        if patch.patterns > 0 {
            rows.push(("patterns", crate::ui::commas(patch.patterns)));
            rows.push((
                "chain",
                format!("{} × {}", crate::ui::commas(patch.chain), patch.loops),
            ));
        }
        rows.push(("steps", crate::ui::commas(patch.steps)));
        rows.push(("events", crate::ui::commas(patch.timeline.events.len())));
        rows.push((
            "length",
            format!(
                "{:.1} s",
                patch.timeline.total_samples as f64 / sample_rate as f64
            ),
        ));

        crate::ui::pairs(&format!("sequence  ({})", path.display()), &rows, true);
        println!();
    }

    for (name, report) in &patch.voices {
        crate::ui::pairs(
            &format!("voice {name}  (rate {sample_rate})"),
            &[
                ("peak", format!("{:.3}", report.peak)),
                ("dc", format!("{:+.4}", report.dc)),
                ("seam", format!("{:.4}", report.seam)),
                ("clamped", crate::ui::commas(report.clamped)),
            ],
            true,
        );
        println!();

        crate::ui::columns(
            "envelope",
            &["segment", "samples", "duration"],
            vec![
                vec![
                    "attack".into(),
                    crate::ui::commas(report.attack),
                    ms(report.attack),
                ],
                vec![
                    "decay".into(),
                    crate::ui::commas(report.decay),
                    ms(report.decay),
                ],
                vec![
                    "sustain".into(),
                    "—".into(),
                    format!("{:.3}", report.sustain_level),
                ],
                vec![
                    "release".into(),
                    crate::ui::commas(report.release),
                    ms(report.release),
                ],
            ],
        );

        for warning in &report.warnings {
            println!("  warn  {name}: {warning}");
            clean = false;
        }
        println!();
    }

    if clean {
        println!("  ok");
    }
    Ok(clean)
}


/// A loaded file: a playable timeline plus one report per voice in it.
pub struct Patch {
    pub timeline: crate::audio::Timeline,
    pub bpm: f32,
    pub division: f32,
    pub step_ms: f32,
    pub steps: usize,
    pub patterns: usize,
    pub chain: usize,
    pub loops: usize,
    pub voices: Vec<(String, Report)>,
}

#[instrument(skip(spec))]
fn bake_voice(spec: &mlua::Table, sample_rate: u32) -> anyhow::Result<(Voice, Report)> {
    let wave_fn: mlua::Function = spec
        .get("waveform")
        .context("voice field `waveform` is missing or not a function")?;
    let env: mlua::Table = spec
        .get("envelope")
        .context("voice field `envelope` is missing or not a table")?;

    let mut clamped = 0usize;
    let mut wave = Vec::with_capacity(TABLE_SIZE);
    {
        let _span = debug_span!("bake_wave", points = TABLE_SIZE).entered();
        for i in 0..TABLE_SIZE {
            let value: f64 = wave_fn
                .call(i as f64 / TABLE_SIZE as f64)
                .context("waveform must return a number for every phase")?;
            let value = value as f32;
            if !(-1.0..=1.0).contains(&value) {
                clamped += 1;
            }
            wave.push(value.clamp(-1.0, 1.0));
        }
    }

    let sustain_level: f64 = env
        .get("sustainLevel")
        .context("envelope.sustainLevel is missing or not a number")?;

    // Rust owns the sample rate, so it sizes each segment and lets the
    // patch's own curve function fill it
    let curve = |ms_key: &str, fn_key: &str, from: f64, to: f64| -> anyhow::Result<Vec<f32>> {
        let ms: f64 = env
            .get(ms_key)
            .with_context(|| format!("envelope.{ms_key} is missing or not a number"))?;
        let shape: mlua::Function = env
            .get(fn_key)
            .with_context(|| format!("envelope.{fn_key} is missing or not a function"))?;
        let steps = (sample_rate as f64 * ms / 1000.0) as usize;
        let filled: mlua::Table = shape.call((from, to, steps))?;

        let mut out = Vec::with_capacity(steps);
        for i in 1..=steps {
            let value: f64 = filled.get(i).with_context(|| {
                format!(
                    "envelope.{fn_key} returned {} of {steps} expected values",
                    i - 1
                )
            })?;
            out.push(value as f32);
        }
        Ok(out)
    };

    let (attack, decay, release) = {
        let _span = debug_span!("bake_envelope").entered();
        (
            curve("attackMS", "attackFunc", 0.0, 1.0)?,
            curve("decayMS", "decayFunc", 1.0, sustain_level)?,
            curve("releaseMS", "releaseFunc", sustain_level, 0.0)?,
        )
    };

    let voice = Voice {
        wave,
        attack,
        decay,
        sustain_level: sustain_level as f32,
        release,
    };
    let report = inspect(&voice, clamped);
    Ok((voice, report))
}

#[instrument]
pub fn load(path: &std::path::Path, sample_rate: u32) -> anyhow::Result<Patch> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    load_source(&source, &path.display().to_string(), sample_rate)
}

/// Same as [`load`] but from a string, so sequences can be tested without
/// fixture files on disk.
pub fn load_source(source: &str, name: &str, sample_rate: u32) -> anyhow::Result<Patch> {
    let lua = mlua::Lua::new();
    lua_register(&lua)?;

    // eval to a Value first: a syntax error and a wrong return type both fail
    // this call, and they deserve different messages
    let returned: mlua::Value = lua.load(source).set_name(name).eval()?;
    let table = match returned {
        mlua::Value::Table(table) => table,
        other => anyhow::bail!("file must return a table, got {}", other.type_name()),
    };

    let osc: mlua::Table = lua.load(r#"return require("synth.osc")"#).eval()?;
    let tuning: mlua::Table = osc.get("tuning")?;
    let to_hz: mlua::Function = tuning.get("frequency")?;

    let voice_specs: Option<mlua::Table> = table.get("voices")?;
    let steps: Option<mlua::Table> = table.get("steps")?;

    // bound rather than returned directly: the match tuple is a temporary
    // that would otherwise outlive `lua` and borrow it past its drop
    let patch = match (voice_specs, steps) {
        (Some(specs), Some(steps)) => {
            sequence(&table, &specs, &steps, &to_hz, sample_rate)
        }
        // a bare voice file: one sustained note, so a two-event timeline
        _ => single(&table, sample_rate),
    };
    patch
}

const SINGLE_HOLD: Duration = Duration::from_millis(1200);

fn single(spec: &mlua::Table, sample_rate: u32) -> anyhow::Result<Patch> {
    let (voice, report) = bake_voice(spec, sample_rate)?;
    let tail = voice.release.len() as u64;
    let hold = (SINGLE_HOLD.as_secs_f64() * sample_rate as f64) as u64;

    Ok(Patch {
        timeline: crate::audio::Timeline {
            voices: vec![std::sync::Arc::new(voice)],
            events: vec![
                (0, crate::audio::Event::On { voice: 0, freq: 440.0 }),
                (hold, crate::audio::Event::Off),
            ],
            total_samples: hold + tail,
        },
        bpm: 0.0,
        division: 0.0,
        step_ms: 0.0,
        steps: 1,
        patterns: 0,
        chain: 0,
        loops: 0,
        voices: vec![("(single note)".to_string(), report)],
    })
}

fn sequence(
    table: &mlua::Table,
    specs: &mlua::Table,
    steps: &mlua::Table,
    to_hz: &mlua::Function,
    sample_rate: u32,
) -> anyhow::Result<Patch> {
    let bpm: f64 = table.get("bpm").context("sequence.bpm is missing")?;
    anyhow::ensure!(bpm > 0.0, "sequence.bpm must be positive, got {bpm}");
    let division: f64 = table.get("division").context("sequence.division is missing")?;
    anyhow::ensure!(
        division > 0.0,
        "sequence.division must be positive, got {division}"
    );
    // one step, in samples. `duration` is measured in steps, not beats, so
    // the grid and the gate length share a unit.
    let step_len = sample_rate as f64 * 60.0 / bpm / division;

    // bake each voice exactly once; steps reference them by index
    let mut voices = Vec::new();
    let mut reports = Vec::new();
    let mut index = std::collections::HashMap::new();
    for pair in specs.clone().pairs::<String, mlua::Table>() {
        let (name, spec) = pair?;
        let (voice, report) = bake_voice(&spec, sample_rate)
            .with_context(|| format!("voice `{name}`"))?;
        index.insert(name.clone(), voices.len());
        voices.push(std::sync::Arc::new(voice));
        reports.push((name, report));
    }
    reports.sort_by(|a, b| a.0.cmp(&b.0));

    // first pass: when does each step's note start
    let count = steps.len()? as usize;
    let mut starts = Vec::new();
    for i in 1..=count {
        let step: mlua::Table = steps.get(i)?;
        let note: Option<u8> = step.get("note")?;
        if note.is_some() {
            starts.push((i, (step_len * (i - 1) as f64) as u64));
        }
    }

    let mut events = Vec::new();
    for (position, &(i, start)) in starts.iter().enumerate() {
        let step: mlua::Table = steps.get(i)?;
        let note: u8 = step.get("note")?;
        let name: String = step
            .get("voice")
            .with_context(|| format!("step {i} has a note but no voice is in effect"))?;
        let voice = *index
            .get(&name)
            .with_context(|| format!("step {i} names voice `{name}`, which is not defined"))?;
        let freq: f64 = to_hz.call(note)?;
        events.push((start, crate::audio::Event::On { voice, freq: freq as f32 }));

        let duration: f64 = step.get("duration")?;
        anyhow::ensure!(
            duration > 0.0,
            "step {i} has duration {duration}; must be positive"
        );
        let ends = start + (step_len * duration) as u64;

        // mono steals, so a long note is cut by the next one. Without this
        // clamp its Off would land later and release the wrong note.
        let next_start = starts.get(position + 1).map(|&(_, at)| at);
        if next_start.is_none_or(|next| ends < next) {
            events.push((ends, crate::audio::Event::Off));
        }
    }

    // explicit rests
    for i in 1..=count {
        let step: mlua::Table = steps.get(i)?;
        let off: Option<bool> = step.get("off")?;
        if off == Some(true) {
            events.push(((step_len * (i - 1) as f64) as u64, crate::audio::Event::Off));
        }
    }

    events.sort_by_key(|(at, _)| *at);
    let tail = voices.iter().map(|v| v.release.len() as u64).max().unwrap_or(0);
    let total_samples = (step_len * count as f64) as u64 + tail;

    Ok(Patch {
        timeline: crate::audio::Timeline {
            voices,
            events,
            total_samples,
        },
        bpm: bpm as f32,
        division: division as f32,
        step_ms: (step_len * 1000.0 / sample_rate as f64) as f32,
        steps: count,
        patterns: table.get("patterns").unwrap_or(0),
        chain: table.get("chain").unwrap_or(0),
        loops: table.get("loop").unwrap_or(1),
        voices: reports,
    })
}

/// Measurements and warnings from a baked patch. Pure data, so the caller
/// decides whether to log them, print them, or fail on them.
#[derive(Debug)]
pub struct Report {
    pub peak: f32,
    pub dc: f32,
    pub seam: f32,
    pub clamped: usize,
    pub attack: usize,
    pub decay: usize,
    pub release: usize,
    pub sustain_level: f32,
    pub warnings: Vec<String>,
}

// each warning corresponds to something audible: flattened peaks, a click
// once per cycle, a speaker cone resting off-center, or a note cut mid-wave
fn inspect(voice: &Voice, clamped: usize) -> Report {
    let wave = &voice.wave;
    let peak = wave.iter().fold(0.0f32, |acc, v| acc.max(v.abs()));
    let dc = wave.iter().sum::<f32>() / wave.len() as f32;
    let seam = (wave[wave.len() - 1] - wave[0]).abs();
    let expected_step = 2.0 / wave.len() as f32;

    let mut warnings = Vec::new();
    if clamped > 0 {
        warnings.push(format!(
            "waveform exceeded -1.0..1.0 at {clamped} points and was flattened"
        ));
    }
    if seam > expected_step * 4.0 {
        warnings.push(format!(
            "waveform does not wrap smoothly (seam {seam:.4}); expect a buzz"
        ));
    }
    if dc.abs() > 0.01 {
        warnings.push(format!("waveform has a DC offset of {dc:.4}"));
    }
    if peak < 0.5 {
        warnings.push(format!("waveform peaks at only {peak:.3}"));
    }
    if voice.release.is_empty() {
        warnings.push("releaseMS = 0 cuts the note mid-cycle and will click".to_string());
    }
    if !(0.0..=1.0).contains(&voice.sustain_level) {
        warnings.push(format!(
            "sustainLevel {} is outside 0.0..1.0",
            voice.sustain_level
        ));
    }

    Report {
        peak,
        dc,
        seam,
        clamped,
        attack: voice.attack.len(),
        decay: voice.decay.len(),
        release: voice.release.len(),
        sustain_level: voice.sustain_level,
        warnings,
    }
}

#[cfg(test)]
mod tests {
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

    fn voice_with(wave: Vec<f32>) -> Voice {
        Voice {
            wave,
            attack: vec![0.0, 0.5],
            decay: vec![1.0],
            sustain_level: 0.7,
            release: vec![0.7, 0.35],
        }
    }

    #[test]
    fn clean_triangle_raises_nothing() {
        let report = inspect(&voice_with(wave_from(triangle, 2048)), 0);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.peak, 1.0);
        assert!(report.dc.abs() < 1e-6);
    }

    #[test]
    fn flags_clamping() {
        let report = inspect(&voice_with(wave_from(triangle, 2048)), 17);
        assert!(report.warnings.iter().any(|w| w.contains("17 points")));
    }

    #[test]
    fn flags_dc_offset() {
        // a triangle lifted entirely above zero
        let report = inspect(&voice_with(wave_from(|p| triangle(p) * 0.4 + 0.5, 2048)), 0);
        assert!(report.warnings.iter().any(|w| w.contains("DC offset")));
    }

    #[test]
    fn flags_wrap_discontinuity() {
        // a saw ends at +1 and restarts at -1: a step of 2.0 every cycle
        let report = inspect(&voice_with(wave_from(|p| 2.0 * p - 1.0, 2048)), 0);
        assert!(report.warnings.iter().any(|w| w.contains("wrap smoothly")));
    }

    #[test]
    fn flags_weak_peak() {
        let report = inspect(&voice_with(wave_from(|p| triangle(p) * 0.2, 2048)), 0);
        assert!(report.warnings.iter().any(|w| w.contains("peaks at only")));
    }

    #[test]
    fn flags_zero_release_and_bad_sustain() {
        let mut voice = voice_with(wave_from(triangle, 2048));
        voice.release = vec![];
        voice.sustain_level = 1.5;
        let report = inspect(&voice, 0);
        assert!(report.warnings.iter().any(|w| w.contains("releaseMS = 0")));
        assert!(report.warnings.iter().any(|w| w.contains("outside 0.0..1.0")));
    }

    // 48000 Hz at 60 bpm makes one beat exactly 48000 samples
    fn seq(steps: &str) -> Patch {
        let source = format!(
            r#"
            local sequence = require("synth.sequence")
            local voice = require("synth.voice")
            local osc = require("synth.osc")
            return sequence.new({{
                bpm = 60,
                voices = {{ a = voice.new({{}}), b = voice.new({{}}) }},
                steps = {steps},
            }})
            "#
        );
        load_source(&source, "test", 48000).unwrap()
    }

    #[test]
    fn steps_land_one_beat_apart() {
        let patch = seq(r#"{ { note = 69, voice = "a" }, {}, { note = 69 } }"#);
        let ons: Vec<u64> = patch
            .timeline
            .events
            .iter()
            .filter(|(_, e)| matches!(e, crate::audio::Event::On { .. }))
            .map(|(at, _)| *at)
            .collect();

        // step 1 at sample 0, step 3 two beats later; the rest emits nothing
        assert_eq!(ons, vec![0, 96_000]);
    }

    #[test]
    fn duration_sets_the_off_position() {
        let patch = seq(r#"{ { note = 69, voice = "a", duration = 0.25 } }"#);
        let offs: Vec<u64> = patch
            .timeline
            .events
            .iter()
            .filter(|(_, e)| matches!(e, crate::audio::Event::Off))
            .map(|(at, _)| *at)
            .collect();

        assert_eq!(offs, vec![12_000]); // a quarter of one beat
    }

    #[test]
    fn a_long_note_emits_no_off_because_the_next_note_steals_it() {
        // duration 3 would end at beat 3, well past the note on beat 1. Left
        // unclamped that Off would release the *second* note instead.
        let patch = seq(r#"{ { note = 69, voice = "a", duration = 3 }, { note = 72 } }"#);

        let offs = patch
            .timeline
            .events
            .iter()
            .filter(|(_, e)| matches!(e, crate::audio::Event::Off))
            .count();
        assert_eq!(offs, 1, "only the final note should emit an off");

        let last = patch.timeline.events.last().unwrap();
        assert!(matches!(last.1, crate::audio::Event::Off));
        assert_eq!(last.0, 48_000 + 24_000); // beat 1 plus half a beat
    }

    #[test]
    fn voice_sticks_until_a_step_changes_it() {
        let patch = seq(r#"{ { note = 69, voice = "a" }, { note = 71 }, { note = 72, voice = "b" } }"#);
        let used: Vec<usize> = patch
            .timeline
            .events
            .iter()
            .filter_map(|(_, e)| match e {
                crate::audio::Event::On { voice, .. } => Some(*voice),
                crate::audio::Event::Off => None,
            })
            .collect();

        assert_eq!(used[0], used[1], "step 2 inherits step 1's voice");
        assert_ne!(used[1], used[2], "step 3 switches");
    }

    #[test]
    fn an_unknown_voice_name_is_rejected() {
        let source = r#"
            local sequence = require("synth.sequence")
            local voice = require("synth.voice")
            return sequence.new({
                bpm = 60,
                voices = { a = voice.new({}) },
                steps = { { note = 69, voice = "nope" } },
            })
        "#;
        let error = load_source(source, "test", 48000)
            .err()
            .expect("voice `nope` is not defined");
        assert!(
            format!("{error:#}").contains("not defined"),
            "unexpected: {error:#}"
        );
    }

    #[test]
    fn midi_note_69_bakes_to_440_hz() {
        let patch = seq(r#"{ { note = 69, voice = "a" } }"#);
        match patch.timeline.events[0].1 {
            crate::audio::Event::On { freq, .. } => assert!((freq - 440.0).abs() < 0.01),
            _ => panic!("expected a note on first"),
        }
    }

    fn one_voice(attack_ms: u32, rate: u32) -> Patch {
        let source = format!(
            r#"
            local voice = require("synth.voice")
            local osc = require("synth.osc")
            return voice.new({{
                waveform = osc.waveform.triangle,
                envelope = osc.envelope.new({{
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
            local voice = require("synth.voice")
            return sequence.new({{
                bpm = 60,
                voice = "a",
                voices = {{ a = voice.new({{}}), b = voice.new({{}}) }},
                {body}
            }})
            "#
        );
        load_source(&source, "test", 48000).unwrap()
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
    fn a_reused_pattern_resolves_to_the_same_voice_each_time() {
        // `y` sets voice "b"; `x` must still get the sequence default both
        // times it appears, not inherit "b" from the pass before it
        let patch = arranged(
            r#"
            patterns = {
                x = { { note = 60 } },
                y = { { note = 70, voice = "b" } },
            },
            chain = { "x", "y", "x" },
            "#,
        );
        let used: Vec<usize> = patch
            .timeline
            .events
            .iter()
            .filter_map(|(_, e)| match e {
                crate::audio::Event::On { voice, .. } => Some(*voice),
                crate::audio::Event::Off => None,
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

    #[test]
    fn bakes_the_triangle_landmarks() {
        let patch = one_voice(10, 48000);
        let wave = &patch.timeline.voices[0].wave;
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
        assert_eq!(one_voice(10, 48000).timeline.voices[0].attack.len(), 480);
        assert_eq!(one_voice(10, 44100).timeline.voices[0].attack.len(), 441);
    }

    // the only thing worth asserting about a file the author tunes by ear is
    // that it still loads without warnings
    #[test]
    fn the_shipped_example_loads_clean() {
        let patch = load(Path::new("lua/example.lua"), 48000).unwrap();

        assert!(patch.bpm > 0.0, "example should be a sequence");
        assert!(!patch.voices.is_empty());
        assert!(!patch.timeline.events.is_empty());
        for (name, report) in &patch.voices {
            assert!(
                report.warnings.is_empty(),
                "voice {name}: {:?}",
                report.warnings
            );
        }
    }
}
