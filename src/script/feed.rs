use anyhow::Context;
use std::time::Duration;

use super::bake::{bake_instrument, InstrumentSpec};
use super::field;
use super::Patch;

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
    instrument: Option<String>,
    duration: f64,
}

const SINGLE_HOLD: Duration = Duration::from_millis(1200);

/// A4, for a bare instrument file with no sequence to say otherwise
const SINGLE_NOTE: u8 = 69;

pub(super) fn single(
    spec: InstrumentSpec,
    sample_rate: u32,
    freqs: Vec<f32>,
    keymap: crate::input::keyboard::Keymap,
) -> anyhow::Result<Patch> {
    let (instrument, report) = bake_instrument(spec, sample_rate)?;
    let tail = instrument.release.len() as u64;
    let hold = (SINGLE_HOLD.as_secs_f64() * sample_rate as f64) as u64;

    Ok(Patch {
        timeline: crate::audio::Timeline {
            instruments: vec![std::sync::Arc::new(instrument)],
            events: vec![
                (
                    0,
                    crate::audio::Event::NoteOn {
                        instrument: 0,
                        note: SINGLE_NOTE,
                        velocity: 127,
                    },
                ),
                (
                    hold,
                    crate::audio::Event::NoteOff {
                        instrument: 0,
                        note: SINGLE_NOTE,
                    },
                ),
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
        reports: vec![("(single note)".to_string(), report)],
    })
}

pub(super) fn sequence(
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
    let division: f64 = table
        .get(field::DIVISION)
        .context("sequence.division is missing")?;
    anyhow::ensure!(
        division > 0.0,
        "sequence.division must be positive, got {division}"
    );
    // one step, in samples. `duration` is measured in steps, not beats, so
    // the grid and the gate length share a unit.
    let step_len = sample_rate as f64 * 60.0 / bpm / division;

    // bake each instrument exactly once; steps reference them by index
    let mut instruments = Vec::new();
    let mut reports = Vec::new();
    let mut index = std::collections::HashMap::new();
    for pair in specs.pairs::<String, mlua::Value>() {
        let (name, spec) = pair?;
        // converted per entry rather than by pairs(),
        // so a bad spec is reported under its instrument name
        let (instrument, report) = lua
            .unpack(spec)
            .map_err(anyhow::Error::from)
            .and_then(|spec| bake_instrument(spec, sample_rate))
            .with_context(|| format!("instrument `{name}`"))?;
        index.insert(name.clone(), instruments.len());
        instruments.push(std::sync::Arc::new(instrument));
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
            .instrument
            .as_deref()
            .with_context(|| format!("step {} has a note but no instrument is in effect", i + 1))?;
        let instrument = *index.get(name).with_context(|| {
            format!(
                "step {} names instrument `{name}`, which is not defined",
                i + 1
            )
        })?;

        events.push((
            start,
            crate::audio::Event::NoteOn {
                instrument,
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
            events.push((ends, crate::audio::Event::NoteOff { instrument, note }));
        }
    }

    // explicit rests release whatever the previous note was
    let mut previous = None;
    for (i, step) in parsed.iter().enumerate() {
        if let Some(note) = step.note {
            // the loop above already rejected a note with no known instrument
            let instrument = step.instrument.as_deref().and_then(|name| index.get(name));
            previous = instrument.map(|&instrument| (instrument, note));
        }
        if step.off {
            if let Some((instrument, note)) = previous {
                events.push((
                    (step_len * i as f64) as u64,
                    crate::audio::Event::NoteOff { instrument, note },
                ));
            }
        }
    }

    events.sort_by_key(|(at, _)| *at);
    let tail = instruments
        .iter()
        .map(|v| v.release.len() as u64)
        .max()
        .unwrap_or(0);
    let total_samples = (step_len * parsed.len() as f64) as u64 + tail;

    Ok(Patch {
        timeline: crate::audio::Timeline {
            instruments,
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
        reports,
    })
}
