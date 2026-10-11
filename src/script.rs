use anyhow::Context;
use std::time::Duration;
use tracing::{debug_span, instrument, warn};

use crate::audio::Voice;

const LUA_SLOPE: &str = include_str!("../lua/slope.lua");
const LUA_WAVE: &str = include_str!("../lua/wave.lua");
const LUA_ENVELOPE: &str = include_str!("../lua/envelope.lua");
const LUA_TUNING: &str = include_str!("../lua/tuning.lua");
const LUA_VOICE: &str = include_str!("../lua/voice.lua");
const LUA_SEQUENCE: &str = include_str!("../lua/sequence.lua");
const LUA_KEYBOARD: &str = include_str!("../lua/keyboard.lua");

// points per waveform cycle: enough that the lowest note still reads
// most of them, small enough that 8 KB stays in L1
const TABLE_SIZE: usize = 2048;


// require checks package.preload before any filesystem search, so the
// embedded libraries resolve with no cwd dependency
fn lua_register(lua: &mlua::Lua) -> anyhow::Result<()> {
    let package: mlua::Table = lua.globals().get(field::PACKAGE)?;
    let preload: mlua::Table = package.get(field::PRELOAD)?;

    for (name, src) in [
        (module::SLOPE, LUA_SLOPE),
        (module::WAVE, LUA_WAVE),
        (module::ENVELOPE, LUA_ENVELOPE),
        (module::TUNING, LUA_TUNING),
        (module::VOICE, LUA_VOICE),
        (module::SEQUENCE, LUA_SEQUENCE),
        (module::KEYBOARD, LUA_KEYBOARD),
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


// The Lua/Rust field contract, in one place. Lua constructors pass unknown
// fields through rather than discarding them, so a misspelling arrives here
// and gets named instead of silently taking a default.
/// One resolved step. The struct is the schema: field names come from here,
/// and `deny_unknown_fields` turns a misspelling in a patch into a named
/// error rather than a silent rest.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    #[serde(default)]
    note: Option<u8>,
    #[serde(default)]
    off: bool,
    voice: Option<String>,
    duration: f64,
}

/// Names the embedded modules are registered under. Used both to register
/// them and to require them, so the two cannot drift.
mod module {
    pub const SLOPE: &str = "synth.slope";
    pub const WAVE: &str = "synth.wave";
    pub const ENVELOPE: &str = "synth.envelope";
    pub const TUNING: &str = "synth.tuning";
    pub const VOICE: &str = "synth.voice";
    pub const SEQUENCE: &str = "synth.sequence";
    pub const KEYBOARD: &str = "synth.keyboard";
}

mod field {
    pub const REQUIRE: &str = "require";
    pub const NEW: &str = "new";

    pub const WAVEFORM: &str = "waveform";
    pub const ENVELOPE: &str = "envelope";
    pub const KEYBOARD: &str = "keyboard";

    pub const ATTACK_MS: &str = "attackMS";
    pub const ATTACK_FUNC: &str = "attackFunc";
    pub const DECAY_MS: &str = "decayMS";
    pub const DECAY_FUNC: &str = "decayFunc";
    pub const SUSTAIN_LEVEL: &str = "sustainLevel";
    pub const RELEASE_MS: &str = "releaseMS";
    pub const RELEASE_FUNC: &str = "releaseFunc";

    pub const BPM: &str = "bpm";
    pub const DIVISION: &str = "division";
    pub const VOICES: &str = "voices";
    pub const STEPS: &str = "steps";
    pub const PATTERNS: &str = "patterns";
    pub const CHAIN: &str = "chain";
    pub const LOOP: &str = "loop";

    pub const FREQUENCY: &str = "frequency";

    // Lua's own, not ours: these can never be renamed, but a typo breaks
    // identically, and `PATH` is also written to
    pub const PACKAGE: &str = "package";
    pub const PRELOAD: &str = "preload";
    pub const PATH: &str = "path";
}

// Built from the consts above, so each name is spelled exactly once. Lua
// constructors pass unknown fields through rather than discarding them, so a
// misspelling arrives here and gets named instead of taking a default.
const VOICE_FIELDS: &[&str] = &[field::WAVEFORM, field::ENVELOPE, field::KEYBOARD];
const ENVELOPE_FIELDS: &[&str] = &[
    field::ATTACK_MS,
    field::ATTACK_FUNC,
    field::DECAY_MS,
    field::DECAY_FUNC,
    field::SUSTAIN_LEVEL,
    field::RELEASE_MS,
    field::RELEASE_FUNC,
];

fn reject_unknown(table: &mlua::Table, allowed: &[&str], what: &str) -> anyhow::Result<()> {
    for pair in table.clone().pairs::<mlua::Value, mlua::Value>() {
        let (key, _) = pair?;
        // non-string keys are array indices, which these tables never use
        if let Some(name) = key.as_str() {
            anyhow::ensure!(
                allowed.contains(&name),
                "{what}: unknown field `{name}`; expected one of {}",
                allowed.join(", ")
            );
        }
    }
    Ok(())
}

/// The raw keyboard layout as written in Lua. Field names live here, and
/// camelCase renaming keeps the Lua side idiomatic without string literals.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KeyboardConfig {
    anchor: Anchor,
    offset: i32,
    template: Template,
    octave_up: Vec<String>,
    octave_down: Vec<String>,
    stop: Vec<String>,
}

/// The physical grid, kept nested because rows and their shifted forms are
/// one unit: a template is a layout, not two parallel lists.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    rows: Vec<String>,
    shifted: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    key: String,
    note: i32,
}

fn keymap(
    lua: &mlua::Lua,
    table: &mlua::Table,
) -> anyhow::Result<crate::input::keyboard::Keymap> {
    let config: KeyboardConfig =
        mlua::LuaSerdeExt::from_value(lua, mlua::Value::Table(table.clone()))
            .context("keyboard layout")?;

    anyhow::ensure!(
        config.template.rows.len() == config.template.shifted.len(),
        "a template's rows and shifted must have the same number of rows"
    );
    let anchor_char = config
        .anchor
        .key
        .chars()
        .next()
        .context("keyboard.anchor.key is empty")?;

    // the anchor fixes the grid, so changing `offset` keeps that key on pitch
    let mut base = None;
    for (r, row) in config.template.rows.iter().enumerate() {
        if let Some(c) = row.chars().position(|ch| ch == anchor_char) {
            base = Some(config.anchor.note - r as i32 * config.offset - c as i32);
        }
    }
    let base = base.with_context(|| {
        format!("keyboard.anchor.key '{anchor_char}' does not appear in the template")
    })?;

    let mut notes = std::collections::HashMap::new();
    for (r, (row, up)) in config
        .template
        .rows
        .iter()
        .zip(&config.template.shifted)
        .enumerate()
    {
        anyhow::ensure!(
            row.chars().count() == up.chars().count(),
            "template row {} and its shifted form differ in length",
            r + 1
        );
        for (c, (plain, raised)) in row.chars().zip(up.chars()).enumerate() {
            let note = base + r as i32 * config.offset + c as i32;
            if (0..128).contains(&note) {
                notes.insert(plain, note as u8);
            }
            // the shifted form of a key is the same position, one octave up
            if (0..128).contains(&(note + 12)) {
                notes.insert(raised, (note + 12) as u8);
            }
        }
    }

    // a binding is a character or a key name; an unrecognised name is an
    // error rather than a silently dead key
    let bindings = |list: Vec<String>, what: &str| -> anyhow::Result<Vec<_>> {
        list.iter()
            .map(|name| {
                crate::input::keyboard::binding(name)
                    .with_context(|| format!("keyboard.{what}: unknown key '{name}'"))
            })
            .collect()
    };

    let stop = bindings(config.stop, "stop")?;
    // a live instrument with no way to silence a stuck note is a trap, and
    // quitting is the only other recovery
    anyhow::ensure!(
        !stop.is_empty(),
        "keyboard.stop must name at least one key to silence with"
    );

    Ok(crate::input::keyboard::Keymap {
        notes,
        octave_up: bindings(config.octave_up, "octaveUp")?,
        octave_down: bindings(config.octave_down, "octaveDown")?,
        stop,
    })
}

/// A loaded file: a playable timeline plus one report per voice in it.
pub struct Patch {
    pub timeline: crate::audio::Timeline,
    /// hz for every midi note, baked from Lua so temperament stays there
    pub freqs: Vec<f32>,
    pub keymap: crate::input::keyboard::Keymap,
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
    reject_unknown(spec, VOICE_FIELDS, "voice")?;

    let wave_fn: mlua::Function = spec
        .get(field::WAVEFORM)
        .context("voice field `waveform` is missing or not a function")?;
    let env: mlua::Table = spec
        .get(field::ENVELOPE)
        .context("voice field `envelope` is missing or not a table")?;
    reject_unknown(&env, ENVELOPE_FIELDS, "envelope")?;

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
        .get(field::SUSTAIN_LEVEL)
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
            curve(field::ATTACK_MS, field::ATTACK_FUNC, 0.0, 1.0)?,
            curve(field::DECAY_MS, field::DECAY_FUNC, 1.0, sustain_level)?,
            curve(field::RELEASE_MS, field::RELEASE_FUNC, sustain_level, 0.0)?,
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

/// What `voice.new({})` gives you, for running with no file.
const DEFAULT: &str = r#"
local voice = require("synth.voice")
return voice.new({})
"#;

#[instrument]
pub fn load_default(sample_rate: u32) -> anyhow::Result<Patch> {
    load_source(DEFAULT, "(default)", sample_rate)
}

#[instrument]
pub fn load(path: &std::path::Path, sample_rate: u32) -> anyhow::Result<Patch> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    load_in(
        &source,
        &path.display().to_string(),
        sample_rate,
        path.parent(),
    )
}

/// Same as [`load`] but from a string, so sequences can be tested without
/// fixture files on disk.
pub fn load_source(source: &str, name: &str, sample_rate: u32) -> anyhow::Result<Patch> {
    load_in(source, name, sample_rate, None)
}

fn load_in(
    source: &str,
    name: &str,
    sample_rate: u32,
    dir: Option<&std::path::Path>,
) -> anyhow::Result<Patch> {
    let lua = mlua::Lua::new();
    lua_register(&lua)?;

    // a patch's own requires resolve next to the patch, not next to the
    // shell's cwd. built through the package table rather than by splicing a
    // path into Lua source, which a path containing a quote would break.
    if let Some(dir) = dir {
        let package: mlua::Table = lua.globals().get(field::PACKAGE)?;
        let existing: String = package.get(field::PATH)?;
        let dir = dir.display();
        package.set(field::PATH, format!("{dir}/?.lua;{dir}/?/init.lua;{existing}"))?;
    }

    // eval to a Value first: a syntax error and a wrong return type both fail
    // this call, and they deserve different messages
    let returned: mlua::Value = lua.load(source).set_name(name).eval()?;
    let table = match returned {
        mlua::Value::Table(table) => table,
        other => anyhow::bail!("file must return a table, got {}", other.type_name()),
    };

    let require: mlua::Function = lua.globals().get(field::REQUIRE)?;
    let tuning: mlua::Table = require.call(module::TUNING)?;
    let to_hz: mlua::Function = tuning.get(field::FREQUENCY)?;

    let mut freqs = Vec::with_capacity(128);
    for note in 0..128 {
        let hz: f64 = to_hz.call(note)?;
        freqs.push(hz as f32);
    }

    // a patch may ship its own layout; otherwise the embedded default
    let layout: mlua::Table = match table.get(field::KEYBOARD)? {
        Some(table) => table,
        // the module exposes a constructor now, so the default layout is
        // keyboard.new() rather than the module table itself
        None => {
            let keyboard: mlua::Table = require.call(module::KEYBOARD)?;
            let new: mlua::Function = keyboard.get(field::NEW)?;
            new.call(())?
        }
    };
    let keys = keymap(&lua, &layout)?;

    let voice_specs: Option<mlua::Table> = table.get(field::VOICES)?;
    let steps: Option<mlua::Table> = table.get(field::STEPS)?;

    // bound rather than returned directly: the match tuple is a temporary
    // that would otherwise outlive `lua` and borrow it past its drop
    let patch = match (voice_specs, steps) {
        (Some(specs), Some(steps)) => {
            sequence(&lua, &table, &specs, &steps, sample_rate, freqs, keys)
        }
        // a bare voice file: one sustained note, so a two-event timeline
        _ => single(&table, sample_rate, freqs, keys),
    };
    patch
}

const SINGLE_HOLD: Duration = Duration::from_millis(1200);
/// A4, for a bare voice file with no sequence to say otherwise
const SINGLE_NOTE: u8 = 69;

fn single(
    spec: &mlua::Table,
    sample_rate: u32,
    freqs: Vec<f32>,
    keymap: crate::input::keyboard::Keymap,
) -> anyhow::Result<Patch> {
    let (voice, report) = bake_voice(spec, sample_rate)?;
    let tail = voice.release.len() as u64;
    let hold = (SINGLE_HOLD.as_secs_f64() * sample_rate as f64) as u64;

    Ok(Patch {
        timeline: crate::audio::Timeline {
            voices: vec![std::sync::Arc::new(voice)],
            events: vec![
                (
                    0,
                    crate::audio::Event::NoteOn {
                        voice: 0,
                        note: SINGLE_NOTE,
                        velocity: 127,
                    },
                ),
                (hold, crate::audio::Event::NoteOff { note: SINGLE_NOTE }),
            ],
            total_samples: hold + tail,
        },
        freqs,
        keymap,
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
    lua: &mlua::Lua,
    table: &mlua::Table,
    specs: &mlua::Table,
    steps: &mlua::Table,
    sample_rate: u32,
    freqs: Vec<f32>,
    keymap: crate::input::keyboard::Keymap,
) -> anyhow::Result<Patch> {
    let bpm: f64 = table.get(field::BPM).context("sequence.bpm is missing")?;
    anyhow::ensure!(bpm > 0.0, "sequence.bpm must be positive, got {bpm}");
    let division: f64 = table.get(field::DIVISION).context("sequence.division is missing")?;
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

    // deserialized once, then walked three times: the struct replaces a
    // dozen get("...") calls and validates every field name
    let parsed: Vec<Step> = mlua::LuaSerdeExt::from_value(lua, mlua::Value::Table(steps.clone()))
        .context("sequence steps")?;

    let starts: Vec<(usize, u64)> = parsed
        .iter()
        .enumerate()
        .filter(|(_, step)| step.note.is_some())
        .map(|(i, _)| (i, (step_len * i as f64) as u64))
        .collect();

    let mut events = Vec::new();
    for (position, &(i, start)) in starts.iter().enumerate() {
        let step = &parsed[i];
        let note = step.note.expect("filtered to steps with notes");
        let name = step
            .voice
            .as_deref()
            .with_context(|| format!("step {} has a note but no voice is in effect", i + 1))?;
        let voice = *index
            .get(name)
            .with_context(|| format!("step {} names voice `{name}`, which is not defined", i + 1))?;

        events.push((
            start,
            crate::audio::Event::NoteOn {
                voice,
                note,
                velocity: 127,
            },
        ));

        anyhow::ensure!(
            step.duration > 0.0,
            "step {} has duration {}; must be positive",
            i + 1,
            step.duration
        );
        let ends = start + (step_len * step.duration) as u64;

        // mono steals, so a long note is cut by the next one. Without this
        // clamp its Off would land later and release the wrong note.
        let next_start = starts.get(position + 1).map(|&(_, at)| at);
        if next_start.is_none_or(|next| ends < next) {
            events.push((ends, crate::audio::Event::NoteOff { note }));
        }
    }

    // explicit rests release whatever the previous note was
    let mut previous = None;
    for (i, step) in parsed.iter().enumerate() {
        if let Some(note) = step.note {
            previous = Some(note);
        }
        if step.off {
            if let Some(note) = previous {
                events.push((
                    (step_len * i as f64) as u64,
                    crate::audio::Event::NoteOff { note },
                ));
            }
        }
    }

    events.sort_by_key(|(at, _)| *at);
    let tail = voices.iter().map(|v| v.release.len() as u64).max().unwrap_or(0);
    let total_samples = (step_len * parsed.len() as f64) as u64 + tail;

    Ok(Patch {
        timeline: crate::audio::Timeline {
            voices,
            events,
            total_samples,
        },
        freqs,
        keymap,
        bpm: bpm as f32,
        division: division as f32,
        step_ms: (step_len * 1000.0 / sample_rate as f64) as f32,
        steps: parsed.len(),
        patterns: table.get(field::PATTERNS).unwrap_or(0),
        chain: table.get(field::CHAIN).unwrap_or(0),
        loops: table.get(field::LOOP).unwrap_or(1),
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
            local wave = require("synth.wave")
            local envelope = require("synth.envelope")
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
            .filter(|(_, e)| matches!(e, crate::audio::Event::NoteOn { .. }))
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
            .filter(|(_, e)| matches!(e, crate::audio::Event::NoteOff { .. }))
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
            .filter(|(_, e)| matches!(e, crate::audio::Event::NoteOff { .. }))
            .count();
        assert_eq!(offs, 1, "only the final note should emit an off");

        let last = patch.timeline.events.last().unwrap();
        assert!(matches!(last.1, crate::audio::Event::NoteOff { .. }));
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
                crate::audio::Event::NoteOn { voice, .. } => Some(*voice),
                crate::audio::Event::NoteOff { .. } => None,
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
    fn a_step_emits_its_midi_note() {
        let patch = seq(r#"{ { note = 69, voice = "a" } }"#);
        match patch.timeline.events[0].1 {
            crate::audio::Event::NoteOn { note, .. } => assert_eq!(note, 69),
            _ => panic!("expected a note on first"),
        }
    }

    fn one_voice(attack_ms: u32, rate: u32) -> Patch {
        let source = format!(
            r#"
            local voice = require("synth.voice")
            local wave = require("synth.wave")
            local envelope = require("synth.envelope")
            return voice.new({{
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
    fn the_anchor_key_lands_on_its_note() {
        let patch = one_voice(10, 48000);
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
            local voice = require("synth.voice")
            local wave = require("synth.wave")
            local envelope = require("synth.envelope")
            local keyboard = require("synth.keyboard")
            return voice.new({
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
            local voice = require("synth.voice")
            local keyboard = require("synth.keyboard")
            return voice.new({
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
        let patch = one_voice(10, 48000);
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
                crate::audio::Event::NoteOn { voice, .. } => Some(*voice),
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
            local voice = require("synth.voice")
            local wave = require("synth.wave")
            local envelope = require("synth.envelope")
            return voice.new({{
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
        let voice = &patch.timeline.voices[0];
        (voice.attack.clone(), voice.release.clone())
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
            local voice = require("synth.voice")
            local envelope = require("synth.envelope")
            return voice.new({
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
