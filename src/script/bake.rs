use std::ops::RangeInclusive;

use anyhow::Context;
use tracing::{debug_span, instrument};

use crate::spec::{Diagnostic, DiagnosticKind, Instrument, Report, Segment};

use super::{field, schema};

// points per waveform cycle: enough that every note above 23.4 Hz
// advances at least one entry per sample, small enough that 8 KB
// stays in L1
const TABLE_SIZE: usize = 2048;

// points per envelope curve, plus one for t = 1, whatever the stage's
// length; render reads between them
const CURVE_POINTS: usize = 1024;

// An instrument table as Rust reads it. The struct is the schema: serde
// cannot be, since its data model has no function type.
pub(super) struct InstrumentSpec {
    waveform: mlua::Function,
    envelope: EnvelopeSpec,
    voice_count: usize,
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
        schema::INSTRUMENT
            .check(&table, None)
            .map_err(|error| mlua::Error::RuntimeError(format!("{error:#}")))?;
        Ok(Self {
            waveform: spec_entry(&table, "instrument", field::WAVEFORM, "function")?,
            // no wrapping: EnvelopeSpec names its own bad field
            envelope: table.get(field::ENVELOPE)?,
            voice_count: spec_entry(&table, "instrument", field::VOICE_COUNT, "count")?,
        })
    }
}

impl mlua::FromLua for EnvelopeSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, "envelope")?;
        schema::ENVELOPE
            .check(&table, None)
            .map_err(|error| mlua::Error::RuntimeError(format!("{error:#}")))?;
        let what = "envelope";
        Ok(Self {
            attack_ms: spec_entry(&table, what, field::ATTACK, "number")?,
            attack_func: spec_entry(&table, what, field::ATTACK_CURVE, "function")?,
            decay_ms: spec_entry(&table, what, field::DECAY, "number")?,
            decay_func: spec_entry(&table, what, field::DECAY_CURVE, "function")?,
            sustain_level: spec_entry(&table, what, field::SUSTAIN, "number")?,
            release_ms: spec_entry(&table, what, field::RELEASE, "number")?,
            release_func: spec_entry(&table, what, field::RELEASE_CURVE, "function")?,
        })
    }
}

pub(super) fn spec_error(from: &'static str, what: &str, message: String) -> mlua::Error {
    mlua::Error::FromLuaConversionError {
        from,
        to: what.to_string(),
        message: Some(message),
    }
}

pub(super) fn spec_table(value: mlua::Value, what: &str) -> mlua::Result<mlua::Table> {
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
pub(super) fn spec_entry<T: mlua::FromLua>(
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
pub(super) fn expect_only(table: &mlua::Table, what: &str, allowed: &[&str]) -> mlua::Result<()> {
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

    // phase 1 is phase 0 again, so a cycle stops one point short of it
    let (cycle, clamped) = {
        let _span = debug_span!("bake_wave", points = TABLE_SIZE).entered();
        let phases = (0..TABLE_SIZE).map(|i| i as f64 / TABLE_SIZE as f64);
        sample(phases, -1.0..=1.0, |phase| {
            spec.waveform
                .call(phase)
                .context("waveform must return a number for every phase")
        })?
    };

    // each curve is sampled at the same points whatever its stage's
    // length; render stretches it over the stage and scales it from the
    // stage's start level to its end
    let curve = |ms: f64, shape: &mlua::Function, name: &str| -> anyhow::Result<(Segment, usize)> {
        let ts = (0..=CURVE_POINTS).map(|i| i as f64 / CURVE_POINTS as f64);
        let (curve, clamped) = sample(ts, 0.0..=1.0, |t| {
            let along = match shape.call::<mlua::Value>(t)? {
                mlua::Value::Number(n) => n,
                mlua::Value::Integer(n) => n as f64,
                // `synth.slope.exponential` handed over without its (),
                // which then returns a curve rather than a point on one
                mlua::Value::Function(_) => anyhow::bail!(
                    "envelope.{name} returned a function, not a number: a curve \
                     constructor needs its (), as in synth.slope.exponential(2)"
                ),
                other => anyhow::bail!(
                    "envelope.{name} must return a number from 0 to 1, got {}",
                    other.type_name()
                ),
            };
            anyhow::ensure!(
                along.is_finite(),
                "envelope.{name} returned {along} at t = {t}; must be a number from 0 to 1"
            );
            Ok(along)
        })?;
        Ok((Segment { curve, ms }, clamped))
    };

    let ((attack, attack_clamped), (decay, decay_clamped), (release, release_clamped)) = {
        let _span = debug_span!("bake_envelope", points = CURVE_POINTS + 1).entered();
        (
            curve(env.attack_ms, &env.attack_func, field::ATTACK_CURVE)?,
            curve(env.decay_ms, &env.decay_func, field::DECAY_CURVE)?,
            curve(env.release_ms, &env.release_func, field::RELEASE_CURVE)?,
        )
    };

    let instrument = Instrument {
        wave: cycle.into(),
        attack,
        decay,
        sustain_level: env.sustain_level as f32,
        release,
        voice_count: spec.voice_count,
    };
    let curves = [
        (field::ATTACK_CURVE, attack_clamped),
        (field::DECAY_CURVE, decay_clamped),
        (field::RELEASE_CURVE, release_clamped),
    ];
    let report = inspect(&instrument, sample_rate, clamped, &curves);
    Ok((instrument, report))
}

// The one sampler behind every baked table, waveform or curve: `read` at
// each point of `xs`, clamped into `range`. Returns the values and how many
// left the range.
fn sample(
    xs: impl Iterator<Item = f64>,
    range: RangeInclusive<f64>,
    mut read: impl FnMut(f64) -> anyhow::Result<f64>,
) -> anyhow::Result<(Vec<f32>, usize)> {
    let mut out = Vec::with_capacity(xs.size_hint().0);
    let mut clamped = 0usize;
    for x in xs {
        let value = read(x)?;
        if !range.contains(&value) {
            clamped += 1;
        }
        out.push(value.clamp(*range.start(), *range.end()) as f32);
    }
    Ok((out, clamped))
}

// each warning corresponds to something audible: flattened peaks, a click
// once per cycle, a speaker cone resting off-center, or a note cut mid-wave
/// `curves` names each envelope curve and how many of its points left
/// 0..1 and were clamped.
pub(super) fn inspect(
    instrument: &Instrument,
    sample_rate: u32,
    clamped: usize,
    curves: &[(&str, usize)],
) -> Report {
    let rate = f64::from(sample_rate);
    let wave = instrument.wave.frame(0);
    let peak = wave.iter().fold(0.0f32, |acc, v| acc.max(v.abs()));
    let dc = wave.iter().sum::<f32>() / wave.len() as f32;
    let seam = (wave[wave.len() - 1] - wave[0]).abs();
    let expected_step = 2.0 / wave.len() as f32;

    let shape = |message: String| Diagnostic::new(DiagnosticKind::Shape, message);
    let mut diagnostics = Vec::new();
    if clamped > 0 {
        diagnostics.push(shape(format!(
            "waveform exceeded -1.0..1.0 at {clamped} points and was flattened"
        )));
    }
    if seam > expected_step * 4.0 {
        diagnostics.push(shape(format!(
            "waveform does not wrap smoothly (seam {seam:.4}); expect a buzz"
        )));
    }
    if dc.abs() > 0.01 {
        diagnostics.push(shape(format!("waveform has a DC offset of {dc:.4}")));
    }
    if peak < 0.5 {
        diagnostics.push(shape(format!("waveform peaks at only {peak:.3}")));
    }
    for (name, count) in curves.iter().filter(|(_, count)| *count > 0) {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::Range,
            format!("envelope.{name} left 0..1 at {count} points and was clamped"),
        ));
    }
    if instrument.release.samples(rate) == 0 {
        diagnostics.push(shape(
            "release = 0 cuts the note mid-cycle and will click".to_string(),
        ));
    }

    Report {
        peak,
        dc,
        seam,
        clamped,
        attack: instrument.attack.samples(rate),
        decay: instrument.decay.samples(rate),
        release: instrument.release.samples(rate),
        sustain_level: instrument.sustain_level,
        diagnostics,
    }
}
