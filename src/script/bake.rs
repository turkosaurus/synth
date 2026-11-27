use std::ops::RangeInclusive;

use tracing::{debug_span, instrument};

use crate::spec::{Diagnostic, DiagnosticKind, Instrument, Lfo, Report, Segment, Unit, Wavetable};

use super::{field, schema, structure, Error};

// points per waveform cycle: enough that every note above 23.4 Hz
// advances at least one entry per sample, small enough that 8 KB
// stays in L1
const TABLE_SIZE: usize = 2048;

// points per envelope curve, plus one for t = 1, whatever the stage's
// length; render reads between them
const CURVE_POINTS: usize = 1024;

// waves one table may hold: TABLE_SIZE f32 each, so 256 is 2 MB
const MAX_WAVES: usize = 256;

// An instrument table as Rust reads it. The struct is the schema: serde
// cannot be, since its data model has no function type.
pub(super) struct InstrumentSpec {
    oscillator: OscSpec,
    envelope: EnvelopeSpec,
    voice_count: usize,
}

/// An oscillator as Rust reads it: one waveform is a fixed cycle, and
/// several are frames blended by `position`.
pub(super) struct OscSpec {
    waveforms: Vec<mlua::Function>,
    slopes: Vec<mlua::Function>,
    position: f64,
    position_mod: Option<LfoSpec>,
}

struct LfoSpec {
    wave: mlua::Function,
    rate: f64,
    depth: f64,
}

impl mlua::FromLua for LfoSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, "lfo")?;
        schema::LFO
            .check(&table, None)
            .map_err(mlua::Error::external)?;
        Ok(Self {
            wave: spec_entry(&table, "lfo", field::WAVE, "function")?,
            rate: spec_entry(&table, "lfo", field::RATE, "number")?,
            depth: spec_entry(&table, "lfo", field::DEPTH, "number")?,
        })
    }
}

/// An LFO on a frequency, such as `cutoff_mod`: its depth carries a unit.
pub(super) struct FrequencyLfoSpec {
    wave: mlua::Function,
    rate: f64,
    depth: f64,
    unit: Unit,
}

impl mlua::FromLua for FrequencyLfoSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, "lfo")?;
        let what = match structure::made_at(&table)? {
            Some(at) => format!("lfo made at {at}"),
            None => "lfo".to_string(),
        };
        let depth = |problem| mlua::Error::external(Error::FrequencyDepth { what, problem });
        if !table.raw_get::<mlua::Value>(field::DEPTH)?.is_nil() {
            return Err(depth(
                "plain `depth` has no unit on a frequency; use `depth_hz` or `depth_octaves`",
            ));
        }
        schema::LFO_FREQUENCY
            .check(&table, None)
            .map_err(mlua::Error::external)?;
        let hz: Option<f64> = table.get(field::DEPTH_HZ)?;
        let octaves: Option<f64> = table.get(field::DEPTH_OCTAVES)?;
        let (depth, unit) = match (hz, octaves) {
            (Some(hz), None) => (hz, Unit::Hz),
            (None, Some(octaves)) => (octaves, Unit::Octaves),
            (Some(_), Some(_)) => {
                return Err(depth("give `depth_hz` or `depth_octaves`, not both"))
            }
            (None, None) => return Err(depth("needs `depth_hz` or `depth_octaves`")),
        };
        Ok(Self {
            wave: spec_entry(&table, "lfo", field::WAVE, "function")?,
            rate: spec_entry(&table, "lfo", field::RATE, "number")?,
            depth,
            unit,
        })
    }
}

impl FrequencyLfoSpec {
    /// The baked LFO, its depth in `unit`, and how many points clamped.
    pub(super) fn bake(&self) -> Result<(Lfo, Unit, usize), Error> {
        let (lfo, count) = bake_lfo(&self.wave, self.rate, self.depth)?;
        Ok((lfo, self.unit, count))
    }
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
            .map_err(mlua::Error::external)?;
        Ok(Self {
            // no wrapping: OscSpec names its own bad field
            oscillator: table.get(field::OSCILLATOR)?,
            // no wrapping: EnvelopeSpec names its own bad field
            envelope: table.get(field::ENVELOPE)?,
            voice_count: spec_entry(&table, "instrument", field::VOICE_COUNT, "count")?,
        })
    }
}

impl mlua::FromLua for OscSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, "osc")?;
        schema::OSC
            .check(&table, None)
            .map_err(mlua::Error::external)?;
        let what = "osc";
        let waveforms: Vec<mlua::Function> =
            spec_entry(&table, what, field::WAVEFORMS, "list of waveforms")?;
        if waveforms.is_empty() {
            return Err(spec_error(
                "table",
                what,
                "`waveforms` is empty; give at least one".into(),
            ));
        }
        if waveforms.len() > MAX_WAVES {
            return Err(spec_error(
                "table",
                what,
                format!("`waveforms` holds {}; at most {MAX_WAVES}", waveforms.len()),
            ));
        }
        Ok(Self {
            waveforms,
            slopes: spec_entry(&table, what, field::SLOPES, "list of curves")?,
            position: spec_entry(&table, what, field::POSITION, "number")?,
            // no wrapping: LfoSpec names its own bad field
            position_mod: table.get(field::POSITION_MOD)?,
        })
    }
}

impl mlua::FromLua for EnvelopeSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, "envelope")?;
        schema::ENVELOPE
            .check(&table, None)
            .map_err(mlua::Error::external)?;
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
) -> Result<(Instrument, Report), Error> {
    let env = spec.envelope;

    let osc = spec.oscillator;
    let tabled = osc.waveforms.len() > 1;
    // one waveform plays its single frame with no blend, so what would
    // move a blend is set aside here and warned about below
    let mut unused = Vec::new();
    let (waves, slopes, position, position_mod) = match tabled {
        true => (osc.waveforms, osc.slopes, osc.position, osc.position_mod),
        false => {
            if !osc.slopes.is_empty() {
                unused.push(field::SLOPES);
            }
            if osc.position != 0.0 {
                unused.push(field::POSITION);
            }
            if osc.position_mod.is_some() {
                unused.push(field::POSITION_MOD);
            }
            (osc.waveforms, Vec::new(), 0.0, None)
        }
    };

    let mut frames = Vec::with_capacity(waves.len() * TABLE_SIZE);
    let mut clamped = Vec::with_capacity(waves.len());
    {
        let _span = debug_span!("bake_wave", points = TABLE_SIZE, waves = waves.len()).entered();
        for (k, waveform) in waves.iter().enumerate() {
            let (cycle, count) = bake_cycle(waveform, |source| match tabled {
                true => Error::Wave {
                    index: k + 1,
                    source,
                },
                false => Error::Waveform { source },
            })?;
            frames.extend(cycle);
            clamped.push(count);
        }
    }
    let position_mod = position_mod
        .map(|lfo| bake_lfo(&lfo.wave, lfo.rate, lfo.depth))
        .transpose()?;
    let slopes = slopes
        .iter()
        .enumerate()
        .map(|(k, shape)| bake_slope(shape, k + 1))
        .collect::<Result<Vec<_>, _>>()?;

    // each curve is sampled at the same points whatever its stage's
    // length; render stretches it over the stage and scales it from the
    // stage's start level to its end
    let curve = |ms: f64, shape: &mlua::Function, field: &'static str| -> Result<_, Error> {
        let ts = (0..=CURVE_POINTS).map(|i| i as f64 / CURVE_POINTS as f64);
        let (curve, clamped) = sample(ts, 0.0..=1.0, |t| {
            let along = match shape.call::<mlua::Value>(t)? {
                mlua::Value::Number(n) => n,
                mlua::Value::Integer(n) => n as f64,
                // `synth.slope.exponential` handed over without its (),
                // which then returns a curve rather than a point on one
                mlua::Value::Function(_) => return Err(Error::CurveUncalled { field }),
                other => {
                    return Err(Error::CurveType {
                        field,
                        got: other.type_name(),
                    })
                }
            };
            if !along.is_finite() {
                return Err(Error::CurveNotFinite {
                    field,
                    value: along,
                    t,
                });
            }
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

    let wave = Wavetable {
        frames,
        points: TABLE_SIZE,
        slopes: slopes.iter().map(|(curve, _)| curve.clone()).collect(),
        position: position as f32,
        position_mod: position_mod.as_ref().map(|(lfo, _)| lfo.clone()),
    };
    let mut instrument = Instrument::new(
        wave,
        attack,
        decay,
        env.sustain_level as f32,
        release,
        spec.voice_count,
    )?;
    let curves = [
        (field::ATTACK_CURVE, attack_clamped),
        (field::DECAY_CURVE, decay_clamped),
        (field::RELEASE_CURVE, release_clamped),
    ];
    // inspect first, so its warnings describe the cycle as written
    let mut report = inspect(&instrument, sample_rate, &clamped, &curves);
    if let Some((_, count)) = position_mod {
        report.diagnostics.extend(lfo_clamped(count));
    }
    for name in unused {
        report.diagnostics.push(Diagnostic::new(
            DiagnosticKind::Unused,
            format!("osc has one waveform, so `{name}` does nothing"),
        ));
    }
    for (k, (_, count)) in slopes
        .iter()
        .enumerate()
        .filter(|(_, (_, count))| *count > 0)
    {
        report.diagnostics.push(Diagnostic::new(
            DiagnosticKind::Range,
            format!(
                "osc slopes[{}] left 0..1 at {count} points and was clamped",
                k + 1
            ),
        ));
    }
    // each frame on its own, so every wave starts from rest and rising
    for k in 0..instrument.wave().frame_count() {
        if let Some(start) = rising_zero(instrument.wave().frame(k)) {
            instrument.rotate_frame(k, start);
        }
    }
    Ok((instrument, report))
}

// One waveform cycle, TABLE_SIZE points from phase 0; phase 1 is phase 0
// again, so it stops one point short. `error` names which waveform failed.
fn bake_cycle(
    waveform: &mlua::Function,
    error: impl Fn(mlua::Error) -> Error,
) -> Result<(Vec<f32>, usize), Error> {
    let phases = (0..TABLE_SIZE).map(|i| i as f64 / TABLE_SIZE as f64);
    sample(phases, -1.0..=1.0, |phase| {
        waveform
            .call(phase)
            .map_err(|source| error(super::error::trimmed(source)))
    })
}

// The same cycle bake an oscillator gets, rotated to start from rest.
// Also returns how many points clamped.
fn bake_lfo(wave: &mlua::Function, rate: f64, depth: f64) -> Result<(Lfo, usize), Error> {
    let (mut cycle, count) = bake_cycle(wave, |source| Error::LfoWave { source })?;
    if let Some(start) = rising_zero(&cycle) {
        cycle.rotate_left(start);
    }
    let lfo = Lfo {
        cycle,
        rate: rate as f32,
        depth: depth as f32,
    };
    Ok((lfo, count))
}

/// The warning for an LFO wave that left -1..1, if `count` points did.
pub(super) fn lfo_clamped(count: usize) -> Option<Diagnostic> {
    (count > 0).then(|| {
        Diagnostic::new(
            DiagnosticKind::Range,
            format!("lfo wave exceeded -1.0..1.0 at {count} points and was flattened"),
        )
    })
}

// A transition's slope, sampled like an envelope curve: the same points
// whatever the table, read between them at playback.
fn bake_slope(shape: &mlua::Function, index: usize) -> Result<(Vec<f32>, usize), Error> {
    let ts = (0..=CURVE_POINTS).map(|i| i as f64 / CURVE_POINTS as f64);
    sample(ts, 0.0..=1.0, |t| {
        let along = match shape.call::<mlua::Value>(t)? {
            mlua::Value::Number(n) => n,
            mlua::Value::Integer(n) => n as f64,
            mlua::Value::Function(_) => return Err(Error::SlopeUncalled { index }),
            other => {
                return Err(Error::SlopeType {
                    index,
                    got: other.type_name(),
                })
            }
        };
        if !along.is_finite() {
            return Err(Error::SlopeNotFinite {
                index,
                value: along,
                t,
            });
        }
        Ok(along)
    })
}

// Where a cycle first rises through 0: the first point at or above 0 whose
// previous point, wrapping, is below 0. Every note starts here, from rest
// and rising, whatever phase the waveform was written from.
pub(super) fn rising_zero(cycle: &[f32]) -> Option<usize> {
    let n = cycle.len();
    (0..n).find(|&i| cycle[(i + n - 1) % n] < 0.0 && cycle[i] >= 0.0)
}

// The one sampler behind every baked table, waveform, curve, or tuning:
// `read` at each point of `xs`, clamped into `range`. Returns the values
// and how many left the range.
pub(super) fn sample(
    xs: impl Iterator<Item = f64>,
    range: RangeInclusive<f64>,
    mut read: impl FnMut(f64) -> Result<f64, Error>,
) -> Result<(Vec<f32>, usize), Error> {
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
/// `clamped` is how many points of each frame left -1..1, and `curves`
/// names each envelope curve and how many of its points left 0..1.
pub(super) fn inspect(
    instrument: &Instrument,
    sample_rate: u32,
    clamped: &[usize],
    curves: &[(&str, usize)],
) -> Report {
    let rate = f64::from(sample_rate);
    let count = instrument.wave().frame_count();
    let shape = |message: String| Diagnostic::new(DiagnosticKind::Shape, message);
    let mut diagnostics = Vec::new();
    // the report's numbers describe the first wave; warnings cover every one
    let mut first = (0.0, 0.0, 0.0);
    for k in 0..count {
        let wave = instrument.wave().frame(k);
        let peak = wave.iter().fold(0.0f32, |acc, v| acc.max(v.abs()));
        let dc = wave.iter().sum::<f32>() / wave.len() as f32;
        let seam = (wave[wave.len() - 1] - wave[0]).abs();
        let expected_step = 2.0 / wave.len() as f32;
        // a square or saw jumps by design; only a wrap rougher than the
        // shape's own steepest step is a mistake
        let steepest = wave
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0f32, f32::max);
        if k == 0 {
            first = (peak, dc, seam);
        }
        let named = |message: String| match count {
            1 => shape(message),
            _ => shape(format!("waves[{}]: {message}", k + 1)),
        };
        let clamped = clamped.get(k).copied().unwrap_or(0);

        if clamped > 0 {
            diagnostics.push(named(format!(
                "waveform exceeded -1.0..1.0 at {clamped} points and was flattened"
            )));
        }
        if seam > expected_step * 4.0 && seam > steepest {
            diagnostics.push(named(format!(
                "waveform does not wrap smoothly (seam {seam:.4}); expect a buzz"
            )));
        }
        if dc.abs() > 0.01 {
            diagnostics.push(named(format!("waveform has a DC offset of {dc:.4}")));
        }
        if rising_zero(wave).is_none() {
            diagnostics.push(named(
                "waveform never rises through 0, so notes start at phase 0 as written".to_string(),
            ));
        }
        if peak < 0.5 {
            diagnostics.push(named(format!("waveform peaks at only {peak:.3}")));
        }
    }
    for (name, count) in curves.iter().filter(|(_, count)| *count > 0) {
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::Range,
            format!("envelope.{name} left 0..1 at {count} points and was clamped"),
        ));
    }
    if instrument.release().samples(rate) == 0 {
        diagnostics.push(shape(
            "release = 0 cuts the note mid-cycle and will click".to_string(),
        ));
    }

    let (peak, dc, seam) = first;
    Report {
        frames: count,
        peak,
        dc,
        seam,
        harmonics: super::spectrum::octaves(instrument.wave().frame(0)),
        clamped: clamped.iter().sum(),
        attack: instrument.attack().samples(rate),
        decay: instrument.decay().samples(rate),
        release: instrument.release().samples(rate),
        sustain_level: instrument.sustain_level(),
        diagnostics,
    }
}
