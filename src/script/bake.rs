use anyhow::Context;
use tracing::{debug_span, instrument};

use crate::audio::Instrument;

use super::field;

// points per waveform cycle: enough that the lowest note still reads
// most of them, small enough that 8 KB stays in L1
const TABLE_SIZE: usize = 2048;

// Which fields an instrument and its envelope may carry. Lua constructors pass
// unknown fields through rather than discarding them, so a misspelling
// arrives here and gets named instead of silently taking a default.
const INSTRUMENT_FIELDS: &[&str] = &[field::WAVEFORM, field::ENVELOPE, field::KEYBOARD];

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
        if let Some(name) = key.as_string().and_then(|s| s.to_str().ok()) {
            anyhow::ensure!(
                allowed.contains(&&*name),
                "{what}: unknown field `{name}`; expected one of {}",
                allowed.join(", ")
            );
        }
    }
    Ok(())
}

#[instrument(skip(spec))]
pub(super) fn bake_instrument(spec: &mlua::Table, sample_rate: u32) -> anyhow::Result<(Instrument, Report)> {
    reject_unknown(spec, INSTRUMENT_FIELDS, "instrument")?;

    let wave_fn: mlua::Function = spec
        .get(field::WAVEFORM)
        .context("instrument field `waveform` is missing or not a function")?;
    let env: mlua::Table = spec
        .get(field::ENVELOPE)
        .context("instrument field `envelope` is missing or not a table")?;
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

    let instrument = Instrument {
        wave,
        attack,
        decay,
        sustain_level: sustain_level as f32,
        release,
    };
    let report = inspect(&instrument, clamped);
    Ok((instrument, report))
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
pub(super) fn inspect(instrument: &Instrument, clamped: usize) -> Report {
    let wave = &instrument.wave;
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
    if instrument.release.is_empty() {
        warnings.push("releaseMS = 0 cuts the note mid-cycle and will click".to_string());
    }
    if !(0.0..=1.0).contains(&instrument.sustain_level) {
        warnings.push(format!(
            "sustainLevel {} is outside 0.0..1.0",
            instrument.sustain_level
        ));
    }

    Report {
        peak,
        dc,
        seam,
        clamped,
        attack: instrument.attack.len(),
        decay: instrument.decay.len(),
        release: instrument.release.len(),
        sustain_level: instrument.sustain_level,
        warnings,
    }
}
