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
        field(field::WAVEFORM, Ty::Function, Bound::Any),
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
        // 0 leaves it to the platform; the device decides the upper limit,
        // checked when it opens
        optional(field::BUFFER_FRAMES, Ty::Integer, Bound::AtLeast(0.0)),
        // fewer slots than a chord's worth of events would drop keys
        optional(field::QUEUE, Ty::Integer, Bound::AtLeast(16.0)),
        // "" is the default folder; anything else is checked when resolved
        optional(field::PATCH_DIR, Ty::Text, Bound::Any),
        optional(field::TUI, Ty::Table, Bound::Any),
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
    ],
};

pub(super) const KEYBOARD: Schema = Schema {
    what: "keyboard",
    fields: &[
        // its name and notes are checked by keymap
        field(field::LAYOUT, Ty::Table, Bound::Any),
        field(field::STOP, Ty::Table, Bound::Any),
        // a zero gate would end each note on the poll after it starts
        field(field::GATE, Ty::Number, Bound::Above(0.0)),
        // zero turns repeat detection off
        field(field::REPEAT_WINDOW, Ty::Number, Bound::AtLeast(0.0)),
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
                | (mlua::Value::Boolean(_), Ty::Flag) => None,
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
