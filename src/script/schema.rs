//! Every value Rust reads from Lua, declared once: its fields, their types,
//! and the ranges they may take.
//!
//! Defaults live in the Lua modules; this decides only what is valid. A patch
//! can override any Lua module, so Lua may choose a value but Rust says
//! whether it is allowed.

use super::{field, Error};

#[derive(Clone, Copy)]
enum Ty {
    Number,
    /// a number with no fractional part
    Integer,
    Function,
    Table,
    Text,
    Flag,
    /// a `synth.osc`, whose stamp the structure walk checks
    Osc,
    /// a `synth.lfo`, whose stamp the structure walk checks
    Lfo,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Self::Number => "a number",
            Self::Integer => "a whole number",
            Self::Function => "a function",
            Self::Table => "a table",
            Self::Text => "a string",
            Self::Flag => "true or false",
            Self::Osc => "a synth.osc({…})",
            Self::Lfo => "a synth.lfo({…})",
        }
    }
}

/// The values a number may take. Bounds are inclusive unless named `Above` or `Below`.
#[derive(Clone, Copy)]
enum Bound {
    Any,
    AtLeast(f64),
    Above(f64),
    Below(f64),
    Between(f64, f64),
}

impl Bound {
    fn holds(self, value: f64) -> bool {
        value.is_finite()
            && match self {
                Self::Any => true,
                Self::AtLeast(low) => value >= low,
                Self::Above(low) => value > low,
                Self::Below(high) => value < high,
                Self::Between(low, high) => (low..=high).contains(&value),
            }
    }

    fn describe(self) -> String {
        match self {
            Self::Any => "finite".to_string(),
            Self::AtLeast(low) => format!("at least {low}"),
            Self::Above(low) => format!("above {low}"),
            Self::Below(high) => format!("below {high}"),
            Self::Between(low, high) => format!("{low} to {high}"),
        }
    }
}

pub(super) struct Field {
    name: &'static str,
    ty: Ty,
    bound: Bound,
    required: bool,
}

const fn field(name: &'static str, ty: Ty, bound: Bound) -> Field {
    Field {
        name,
        ty,
        bound,
        required: true,
    }
}

const fn optional(name: &'static str, ty: Ty, bound: Bound) -> Field {
    Field {
        name,
        ty,
        bound,
        required: false,
    }
}

pub(super) struct Schema {
    what: &'static str,
    fields: &'static [Field],
}

pub(super) const ENVELOPE: Schema = Schema {
    what: "envelope",
    fields: &[
        field(field::ATTACK, Ty::Number, Bound::AtLeast(0.0)),
        field(field::ATTACK_CURVE, Ty::Function, Bound::Any),
        field(field::DECAY, Ty::Number, Bound::AtLeast(0.0)),
        field(field::DECAY_CURVE, Ty::Function, Bound::Any),
        field(field::SUSTAIN, Ty::Number, Bound::Between(0.0, 1.0)),
        field(field::RELEASE, Ty::Number, Bound::AtLeast(0.0)),
        field(field::RELEASE_CURVE, Ty::Function, Bound::Any),
    ],
};

pub(super) const INSTRUMENT: Schema = Schema {
    what: "instrument",
    fields: &[
        field(field::OSCILLATOR, Ty::Osc, Bound::Any),
        field(field::ENVELOPE, Ty::Table, Bound::Any),
        // a pool of zero voices could never sound
        field(field::VOICE_COUNT, Ty::Integer, Bound::AtLeast(1.0)),
        // read by load_in, not by the instrument
        optional(field::KEYBOARD, Ty::Table, Bound::Any),
        optional(field::TUNING, Ty::Function, Bound::Any),
        optional(field::FILTER, Ty::Table, Bound::Any),
    ],
};

pub(super) const CONFIG: Schema = Schema {
    what: "config.lua",
    // every field optional: the built-in file supplies what a user's leaves out
    fields: &[
        // a release, parsed once read
        optional(field::VERSION, Ty::Text, Bound::Any),
        // 0 leaves it to the platform; the device decides the upper limit,
        // checked when it opens
        optional(field::BUFFER_FRAMES, Ty::Integer, Bound::AtLeast(0.0)),
        // fewer slots than a chord's worth of events would drop keys
        optional(field::QUEUE, Ty::Integer, Bound::AtLeast(16.0)),
        // "" is the default folder; anything else is checked when resolved
        optional(field::CUSTOM_DIR, Ty::Text, Bound::Any),
        optional(field::TUI, Ty::Table, Bound::Any),
        optional(field::LOG, Ty::Table, Bound::Any),
    ],
};

pub(super) const CONFIG_LOG: Schema = Schema {
    what: "config.lua log",
    fields: &[
        // KiB; 0 never trims, and a trim reads the kept part into memory
        // once at start, so the ceiling is 1 GiB
        optional(
            field::TRIM_SIZE_KB,
            Ty::Integer,
            Bound::Between(0.0, 1_048_576.0),
        ),
    ],
};

pub(super) const CONFIG_TUI: Schema = Schema {
    what: "config.lua tui",
    fields: &[
        // ms; 0 would spin, and a second between key reads is unusable
        optional(field::POLL, Ty::Number, Bound::Between(1.0, 1000.0)),
        optional(field::METER_HOLD, Ty::Number, Bound::AtLeast(0.0)),
        // the fall rate divides by it
        optional(field::METER_FALL, Ty::Number, Bound::Above(0.0)),
        // the meter's scale divides by it, and 0 dB is full scale
        optional(field::METER_FLOOR, Ty::Number, Bound::Below(0.0)),
        optional(field::METER_LATENCY, Ty::Table, Bound::Any),
        optional(field::METER_FPS, Ty::Table, Bound::Any),
        // one ring per producing thread, allocated when the stream opens;
        // a full ring loses the record, never the count
        optional(field::FAULTS, Ty::Integer, Bound::Between(1.0, 65536.0)),
        optional(field::SOURCE, Ty::Table, Bound::Any),
        optional(field::SCOPE, Ty::Table, Bound::Any),
        optional(field::SPECTRUM, Ty::Table, Bound::Any),
    ],
};

pub(super) const CONFIG_SOURCE: Schema = Schema {
    what: "config.lua tui source",
    fields: &[
        // a name, checked against the streams once read
        optional(field::STREAM, Ty::Text, Bound::Any),
        // 0 is the mix; the device's channel count is checked when it opens
        optional(field::CHANNEL, Ty::Integer, Bound::Between(0.0, 64.0)),
        // ms; one ring per stream, allocated when the session opens and
        // drained every redraw, so it needs only outlast a slow frame
        optional(field::TEE, Ty::Number, Bound::Between(1.0, 10_000.0)),
    ],
};

pub(super) const CONFIG_SCOPE: Schema = Schema {
    what: "config.lua tui scope",
    fields: &[
        // ms; allocated once, and searched for a trigger every redraw
        optional(field::HISTORY, Ty::Number, Bound::Between(1.0, 10_000.0)),
        // a window shorter than a quarter cycle shows no shape
        optional(field::CYCLES, Ty::Number, Bound::Between(0.25, 64.0)),
    ],
};

pub(super) const CONFIG_SPECTRUM: Schema = Schema {
    what: "config.lua tui spectrum",
    fields: &[
        // transformed every redraw on the UI thread; that it is a power of
        // two is checked once read
        optional(field::SIZE, Ty::Integer, Bound::Between(256.0, 32_768.0)),
        // the spectrum's scale divides by it, and 0 dB is full scale
        optional(field::FLOOR, Ty::Number, Bound::Below(0.0)),
        // both panes keep at least a row
        optional(field::HEIGHT, Ty::Number, Bound::Between(1.0, 99.0)),
    ],
};

pub(super) const CONFIG_LATENCY: Schema = Schema {
    what: "config.lua tui meter_latency",
    fields: &[
        optional(field::WINDOW, Ty::Number, Bound::Above(0.0)),
        // one ring, allocated when the session opens; the view drains it
        // every frame, so far fewer than the maximum are ever waiting
        optional(field::RECORDS, Ty::Integer, Bound::Between(1.0, 65536.0)),
        // the log scale divides by both ends; their order is checked once
        // both are known
        optional(field::FLOOR, Ty::Number, Bound::Above(0.0)),
        optional(field::CEIL, Ty::Number, Bound::Above(0.0)),
        optional(field::INPUT, Ty::Table, Bound::Any),
        optional(field::RENDER, Ty::Table, Bound::Any),
        optional(field::OUTPUT, Ty::Table, Bound::Any),
        optional(field::TOTAL, Ty::Table, Bound::Any),
    ],
};

/// A stage's budget in ms, or the view's frame rate thresholds. Their
/// order is checked once both are known, since it differs between them.
pub(super) const CONFIG_BUDGET: Schema = Schema {
    what: "config.lua budget",
    fields: &[
        optional(field::WARN, Ty::Number, Bound::AtLeast(0.0)),
        optional(field::DANGER, Ty::Number, Bound::AtLeast(0.0)),
    ],
};

pub(super) const KEYBOARD: Schema = Schema {
    what: "keyboard",
    fields: &[
        // its name and notes are checked by keymap
        field(field::LAYOUT, Ty::Table, Bound::Any),
        field(field::STOP, Ty::Table, Bound::Any),
        // optional, so a keyboard table written before it still loads
        optional(field::SCOPE, Ty::Table, Bound::Any),
        // a zero gate would end each note on the poll after it starts
        field(field::GATE, Ty::Number, Bound::Above(0.0)),
        // zero turns autorepeat detection off
        field(field::AUTOREPEAT_THRESHOLD, Ty::Number, Bound::AtLeast(0.0)),
        // zero turns the double tap off; past a second it is two taps
        field(
            field::DOUBLETAP_THRESHOLD,
            Ty::Number,
            Bound::Between(0.0, 1000.0),
        ),
        // 0 is MIDI's note-off spelled as a note-on; above 127 is not MIDI
        field(field::VELOCITY, Ty::Integer, Bound::Between(1.0, 127.0)),
        // past 10 octaves every shift leaves MIDI's note range
        field(field::OCTAVE, Ty::Integer, Bound::Between(-10.0, 10.0)),
        field(field::OCTAVE_LOW, Ty::Integer, Bound::Between(0.0, 10.0)),
        field(field::OCTAVE_HIGH, Ty::Integer, Bound::Between(0.0, 10.0)),
    ],
};

/// The table `synth.sequence` builds, not the one a patch passes it: the
/// Lua constructor checks its own input's field names.
pub(super) const SEQUENCE: Schema = Schema {
    what: "sequence",
    fields: &[
        field(field::BPM, Ty::Number, Bound::Above(0.0)),
        field(field::DIVISION, Ty::Number, Bound::Above(0.0)),
        field(field::INSTRUMENTS, Ty::Table, Bound::Any),
        field(field::STEPS, Ty::Table, Bound::Any),
        field(field::PATTERNS, Ty::Integer, Bound::AtLeast(0.0)),
        field(field::CHAIN, Ty::Integer, Bound::AtLeast(0.0)),
        field(field::LOOP, Ty::Integer, Bound::AtLeast(1.0)),
        optional(field::KEYBOARD, Ty::Table, Bound::Any),
        optional(field::TUNING, Ty::Function, Bound::Any),
        optional(field::FILTER, Ty::Table, Bound::Any),
    ],
};

/// A `synth.lfo` on `position`, as its constructor and `synth.osc` fill
/// it. `depth` is in position's units, so 0 to 1.
pub(super) const LFO: Schema = Schema {
    what: "lfo",
    fields: &[
        field(field::WAVE, Ty::Function, Bound::Any),
        field(field::RATE, Ty::Number, Bound::AtLeast(0.0)),
        field(field::DEPTH, Ty::Number, Bound::Between(0.0, 1.0)),
    ],
};

/// A `synth.lfo` on a frequency such as cutoff. Exactly one depth is
/// given, which the reader checks; past the cutoff's range it clamps.
pub(super) const LFO_FREQUENCY: Schema = Schema {
    what: "lfo",
    fields: &[
        field(field::WAVE, Ty::Function, Bound::Any),
        field(field::RATE, Ty::Number, Bound::AtLeast(0.0)),
        optional(field::DEPTH_HZ, Ty::Number, Bound::AtLeast(0.0)),
        optional(field::DEPTH_OCTAVES, Ty::Number, Bound::AtLeast(0.0)),
    ],
};

/// A `synth.osc`, as its constructor fills it.
pub(super) const OSC: Schema = Schema {
    what: "osc",
    fields: &[
        field(field::WAVEFORMS, Ty::Table, Bound::Any),
        field(field::SLOPES, Ty::Table, Bound::Any),
        field(field::POSITION, Ty::Number, Bound::Between(0.0, 1.0)),
        optional(field::POSITION_MOD, Ty::Lfo, Bound::Any),
    ],
};

/// One step as `synth.sequence` resolves it.
pub(super) const STEP: Schema = Schema {
    what: "step",
    fields: &[
        optional(field::NOTE, Ty::Integer, Bound::Between(0.0, 127.0)),
        optional(field::OFF, Ty::Flag, Bound::Any),
        optional(field::INSTRUMENT, Ty::Text, Bound::Any),
        field(field::DURATION, Ty::Number, Bound::Above(0.0)),
        field(field::VELOCITY, Ty::Integer, Bound::Between(1.0, 127.0)),
    ],
};

impl Schema {
    /// Fails on an unknown field, a missing one, a wrong type, or a number
    /// out of range, naming where the value was made when a constructor
    /// stamped it. `name` replaces the schema's own name, as `step 3`.
    pub(super) fn check(&self, table: &mlua::Table, name: Option<&str>) -> Result<(), Error> {
        let what = match (name, super::structure::made_at(table)?) {
            (Some(name), _) => name.to_string(),
            (None, Some(at)) => format!("{} made at {at}", self.what),
            (None, None) => self.what.to_string(),
        };

        for pair in table.pairs::<mlua::Value, mlua::Value>() {
            let (key, _) = pair?;
            // non-string keys are array entries, which these tables never use
            let Some(key) = key.as_string().and_then(|s| s.to_str().ok()) else {
                continue;
            };
            if !self.fields.iter().any(|field| field.name == &*key) {
                return Err(Error::UnknownField {
                    what,
                    field: key.to_string(),
                    expected: self.fields.iter().map(|field| field.name).collect(),
                });
            }
        }

        for field in self.fields {
            let value: mlua::Value = table.raw_get(field.name)?;
            if value.is_nil() {
                if field.required {
                    return Err(Error::MissingField {
                        what,
                        field: field.name,
                    });
                }
                continue;
            }
            let number = match (&value, field.ty) {
                (mlua::Value::Integer(n), Ty::Number | Ty::Integer) => Some(*n as f64),
                (mlua::Value::Number(n), Ty::Number) => Some(*n),
                (mlua::Value::Number(n), Ty::Integer) if n.fract() == 0.0 => Some(*n),
                (mlua::Value::Function(_), Ty::Function)
                | (mlua::Value::Table(_), Ty::Table)
                | (mlua::Value::String(_), Ty::Text)
                | (mlua::Value::Boolean(_), Ty::Flag)
                | (mlua::Value::Table(_), Ty::Osc | Ty::Lfo) => None,
                (_, ty) => {
                    return Err(Error::WrongType {
                        what,
                        field: field.name,
                        expected: ty.name(),
                        got: shown(&value),
                    })
                }
            };
            if let Some(number) = number {
                if !field.bound.holds(number) {
                    return Err(Error::OutOfRange {
                        what,
                        field: field.name,
                        value: number,
                        bound: field.bound.describe(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// A value as an error message shows it: numbers as written, others by type.
fn shown(value: &mlua::Value) -> String {
    match value {
        mlua::Value::Integer(n) => n.to_string(),
        mlua::Value::Number(n) => n.to_string(),
        other => format!("a {}", other.type_name()),
    }
}
