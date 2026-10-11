use anyhow::Context;
use tracing::{debug_span, instrument};

use crate::audio::Instrument;

use super::field;

// points per waveform cycle: enough that every note above 23.4 Hz
// advances at least one entry per sample, small enough that 8 KB
// stays in L1
const TABLE_SIZE: usize = 2048;

// An instrument table as Rust reads it. The struct is the schema: serde
// cannot be, since its data model has no function type.
pub(super) struct InstrumentSpec {
    waveform: mlua::Function,
    envelope: EnvelopeSpec,
}

pub(super) struct EnvelopeSpec {
    attack_ms: f64,
    attack_func: mlua::Function,
    decay_ms: f64,
    decay_func: mlua::Function,
    sustain_level: f64,
    release_ms: f64,
    release_func: mlua::Function,
}

impl mlua::FromLua for InstrumentSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, "instrument")?;
        // keyboard is read by load_in, not here, but is still allowed
        expect_only(
            &table,
            "instrument",
            &[field::WAVEFORM, field::ENVELOPE, field::KEYBOARD],
        )?;
        Ok(Self {
            waveform: spec_entry(&table, "instrument", field::WAVEFORM, "function")?,
            // no wrapping: EnvelopeSpec names its own bad field
            envelope: table.get(field::ENVELOPE)?,
        })
    }
}

impl mlua::FromLua for EnvelopeSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, "envelope")?;
        expect_only(
            &table,
            "envelope",
            &[
                field::ATTACK_MS,
                field::ATTACK_FUNC,
                field::DECAY_MS,
                field::DECAY_FUNC,
                field::SUSTAIN_LEVEL,
                field::RELEASE_MS,
                field::RELEASE_FUNC,
            ],
        )?;
        let what = "envelope";
        Ok(Self {
            attack_ms: spec_entry(&table, what, field::ATTACK_MS, "number")?,
            attack_func: spec_entry(&table, what, field::ATTACK_FUNC, "function")?,
            decay_ms: spec_entry(&table, what, field::DECAY_MS, "number")?,
            decay_func: spec_entry(&table, what, field::DECAY_FUNC, "function")?,
            sustain_level: spec_entry(&table, what, field::SUSTAIN_LEVEL, "number")?,
            release_ms: spec_entry(&table, what, field::RELEASE_MS, "number")?,
            release_func: spec_entry(&table, what, field::RELEASE_FUNC, "function")?,
        })
    }
}

fn spec_error(from: &'static str, what: &str, message: String) -> mlua::Error {
    mlua::Error::FromLuaConversionError {
        from,
        to: what.to_string(),
        message: Some(message),
    }
}

fn spec_table(value: mlua::Value, what: &str) -> mlua::Result<mlua::Table> {
    match value {
        mlua::Value::Table(table) => Ok(table),
        other => Err(spec_error(
            other.type_name(),
            what,
            "expected a table".into(),
        )),
    }
}

// Lua validation
fn spec_entry<T: mlua::FromLua>(
    table: &mlua::Table,
    what: &str,
    name: &str,
    kind: &str,
) -> mlua::Result<T> {
    table.get(name).map_err(|_| {
        spec_error(
            "table",
            what,
            format!("`{name}` is missing or not a {kind}"),
        )
    })
}

// Lua constructors pass unknown fields through rather than discarding them,
// so a misspelling arrives here and gets named instead of taking a default.
fn expect_only(table: &mlua::Table, what: &str, allowed: &[&str]) -> mlua::Result<()> {
    for pair in table.pairs::<mlua::Value, mlua::Value>() {
        let (key, _) = pair?;
        // non-string keys are array indices, which these tables never use
        if let Some(name) = key.as_string().and_then(|s| s.to_str().ok()) {
            if !allowed.contains(&&*name) {
                return Err(spec_error(
                    "table",
                    what,
                    format!(
                        "unknown field `{name}`; expected one of {}",
                        allowed.join(", ")
                    ),
                ));
            }
        }
    }
    Ok(())
}

#[instrument(skip(spec))]
pub(super) fn bake_instrument(
    spec: InstrumentSpec,
    sample_rate: u32,
) -> anyhow::Result<(Instrument, Report)> {
    let env = spec.envelope;

    let mut clamped = 0usize;
    let mut wave = Vec::with_capacity(TABLE_SIZE);
    {
        let _span = debug_span!("bake_wave", points = TABLE_SIZE).entered();
        for i in 0..TABLE_SIZE {
            let value: f64 = spec
                .waveform
                .call(i as f64 / TABLE_SIZE as f64)
                .context("waveform must return a number for every phase")?;
            let value = value as f32;
            if !(-1.0..=1.0).contains(&value) {
                clamped += 1;
            }
            wave.push(value.clamp(-1.0, 1.0));
        }
    }

    // Rust owns the sample rate, so it sizes each segment and lets the
    // patch's own curve function fill it
    let curve = |ms: f64,
                 shape: &mlua::Function,
                 name: &str,
                 from: f64,
                 to: f64|
     -> anyhow::Result<Vec<f32>> {
        let steps = (sample_rate as f64 * ms / 1000.0) as usize;
        let filled: mlua::Table = shape.call((from, to, steps))?;

        let mut out = Vec::with_capacity(steps);
        for i in 1..=steps {
            let value: f64 = filled.get(i).with_context(|| {
                format!(
                    "envelope.{name} returned {} of {steps} expected values",
                    i - 1
                )
            })?;
            out.push(value as f32);
        }
        Ok(out)
    };

    let sustain_level = env.sustain_level;
    let (attack, decay, release) = {
        let _span = debug_span!("bake_envelope").entered();
        (
            curve(
                env.attack_ms,
                &env.attack_func,
                field::ATTACK_FUNC,
                0.0,
                1.0,
            )?,
            curve(
                env.decay_ms,
                &env.decay_func,
                field::DECAY_FUNC,
                1.0,
                sustain_level,
            )?,
            curve(
                env.release_ms,
                &env.release_func,
                field::RELEASE_FUNC,
                sustain_level,
                0.0,
            )?,
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
