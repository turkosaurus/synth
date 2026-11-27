use anyhow::Context;

use super::bake::{bake_instrument, InstrumentSpec};
use super::{field, Error};
use crate::spec::Patch;

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
    /// defaulted in `sequence.lua`, so every resolved step carries one
    velocity: u8,
}

/// The report name of a bare instrument file's one instrument, which sits
/// at the root of the patch rather than under `instruments`.
pub(super) const SINGLE: &str = "(single note)";

/// A bare instrument file: one instrument for the keyboard, and no events.
pub(super) fn bare(
    spec: InstrumentSpec,
    sample_rate: u32,
    freqs: crate::spec::Freqs,
    keymap: crate::spec::Keymap,
) -> anyhow::Result<Patch> {
    let (instrument, report) = bake_instrument(spec, sample_rate)?;

    Ok(Patch {
        timeline: crate::spec::Timeline {
            instruments: vec![instrument],
            events: Vec::new(),
            total_samples: 0,
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
        reports: vec![(SINGLE.to_string(), report)],
        filter: None,
        filter_report: None,
        tuning_diagnostics: Vec::new(),
        printed: Vec::new(),
        structure: Vec::new(),
    })
}

pub(super) fn sequence(
    lua: &mlua::Lua,
    table: &mlua::Table,
    specs: &mlua::Table,
    steps: &mlua::Table,
    sample_rate: u32,
    freqs: crate::spec::Freqs,
    keymap: crate::spec::Keymap,
) -> anyhow::Result<Patch> {
    super::schema::SEQUENCE.check(table, None)?;
    for (i, step) in steps.sequence_values::<mlua::Table>().enumerate() {
        super::schema::STEP.check(&step?, Some(&format!("step {}", i + 1)))?;
    }
    // both checked positive by schema::SEQUENCE
    let bpm: f64 = table.get(field::BPM)?;
    let division: f64 = table.get(field::DIVISION)?;
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
            .map_err(Error::from)
            .and_then(|spec| bake_instrument(spec, sample_rate))
            .with_context(|| format!("instrument `{name}`"))?;
        index.insert(name.clone(), instruments.len());
        instruments.push(instrument);
        reports.push((name, report));
    }
    reports.sort_by(|a, b| a.0.cmp(&b.0));

    // deserialized once, then walked three times: the struct replaces a
    // dozen get("...") calls and validates every field name
    let value = mlua::Value::Table(steps.clone());
    let parsed: Vec<Step> =
        mlua::LuaSerdeExt::from_value(lua, value).map_err(|source| Error::Convert {
            what: "sequence steps",
            source,
        })?;

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
            .ok_or(Error::NoInstrument { step: i + 1 })?;
        let instrument = *index.get(name).ok_or_else(|| Error::UndefinedInstrument {
            step: i + 1,
            name: name.to_string(),
        })?;

        events.push((
            start,
            crate::spec::Event::NoteOn {
                instrument,
                note,
                velocity: step.velocity,
            },
        ));

        let ends = start + (step_len * step.duration) as u64;

        // A retrigger of the same pitch on the same instrument takes this
        // note's voice, so an Off landing after it would release the new
        // note. Any other later note leaves this Off correct: it either
        // releases a voice still sounding or names one already stolen.
        let retrigger = starts[position + 1..]
            .iter()
            .find(|&&(j, _)| {
                parsed[j].note == Some(note) && parsed[j].instrument.as_deref() == Some(name)
            })
            .map(|&(_, at)| at);
        if retrigger.is_none_or(|next| ends < next) {
            events.push((ends, crate::spec::Event::NoteOff { instrument, note }));
        }
    }

    // A rest silences its instrument: every voice releases, like MIDI All
    // Notes Off. With no instrument in effect nothing can be sounding.
    for (i, step) in parsed.iter().enumerate().filter(|(_, step)| step.off) {
        let Some(name) = step.instrument.as_deref() else {
            continue;
        };
        let instrument = *index.get(name).ok_or_else(|| Error::UndefinedInstrument {
            step: i + 1,
            name: name.to_string(),
        })?;
        events.push((
            (step_len * i as f64) as u64,
            crate::spec::Event::AllNotesOff { instrument },
        ));
    }

    events.sort_by_key(|(at, _)| *at);
    let tail = instruments
        .iter()
        .map(|v| v.release().samples(f64::from(sample_rate)) as u64)
        .max()
        .unwrap_or(0);
    let total_samples = (step_len * parsed.len() as f64) as u64 + tail;

    Ok(Patch {
        timeline: crate::spec::Timeline {
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
        filter: None,
        filter_report: None,
        tuning_diagnostics: Vec::new(),
        printed: Vec::new(),
        structure: Vec::new(),
    })
}
