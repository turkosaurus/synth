/// Why a patch failed to load. Each variant names where in the patch it
/// failed, so a caller can match on the reason and still print the place.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("{what}: unknown field `{field}`; expected one of {}", .expected.join(", "))]
    UnknownField {
        what: String,
        field: String,
        expected: Vec<&'static str>,
    },

    #[error("{what}: `{field}` is missing")]
    MissingField { what: String, field: &'static str },

    #[error("{what}: `{field}` must be {expected}, got {got}")]
    WrongType {
        what: String,
        field: &'static str,
        expected: &'static str,
        got: String,
    },

    #[error("{what}: `{field}` is {value}; must be {bound}")]
    OutOfRange {
        what: String,
        field: &'static str,
        value: f64,
        bound: String,
    },

    #[error(
        "{what}: expected {}, got {} made at {at}",
        article(expected),
        article(got)
    )]
    WrongKind {
        what: String,
        expected: &'static str,
        got: String,
        at: String,
    },

    #[error(
        "{what}: expected {}, got a plain table; build it with {}",
        article(expected),
        constructor(expected)
    )]
    PlainTable {
        what: String,
        expected: &'static str,
    },

    #[error("{what}: expected {}, got a {got}", article(expected))]
    NotATable {
        what: String,
        expected: &'static str,
        got: &'static str,
    },

    #[error(
        "a patch must return an instrument or a sequence, got {} made at {at}",
        article(got)
    )]
    RootKind { got: String, at: String },

    #[error(
        "a patch must return synth.instrument({{…}}) or synth.sequence({{…}}), got a plain table"
    )]
    RootPlainTable,

    #[error(
        "{what}: `{field}` applies only to the patch's root; \
         set it on the sequence (instrument made at {at})"
    )]
    RootOnly {
        what: String,
        field: &'static str,
        at: String,
    },

    #[error("{what} contains itself; a patch cannot hold a cycle")]
    Cycle { what: String },

    /// Lua's message follows as the source, as context would add it.
    #[error("{what}")]
    Convert {
        what: &'static str,
        #[source]
        source: mlua::Error,
    },

    #[error("{what}: unknown key '{key}'")]
    UnknownKey { what: String, key: String },

    #[error("keyboard.stop must name at least one key to silence with")]
    NoStopKey,

    #[error("keyboard.layout plays note {note} on '{key}', which keyboard.{what} also binds")]
    KeyConflict {
        what: &'static str,
        key: char,
        note: u8,
    },

    #[error("keyboard.layout.notes: key '{key}' must be one character")]
    NoteKey { key: String },

    #[error("keyboard.layout.notes['{key}'] is {value}; must be a whole number 0 to 127")]
    NoteRange { key: String, value: f64 },

    #[error("tuning must return Hz for note {note}")]
    TuningCall {
        note: u8,
        #[source]
        source: mlua::Error,
    },

    #[error("tuning returned {} Hz for note {note}; must be positive and finite", hz_shown(*hz))]
    TuningHz { note: u8, hz: f64 },

    #[error("waveform must return a number for every phase")]
    Waveform {
        #[source]
        source: mlua::Error,
    },

    #[error("wave.table waves[{index}] must return a number for every phase")]
    Wave {
        index: usize,
        #[source]
        source: mlua::Error,
    },

    #[error(
        "wave.table slopes[{index}] returned a function, not a number: a curve \
         constructor needs its (), as in synth.slope.exponential(2)"
    )]
    SlopeUncalled { index: usize },

    #[error("wave.table slopes[{index}] must return a number from 0 to 1, got {got}")]
    SlopeType { index: usize, got: &'static str },

    #[error(
        "wave.table slopes[{index}] returned {value} at t = {t}; must be a number from 0 to 1"
    )]
    SlopeNotFinite { index: usize, value: f64, t: f64 },

    #[error(
        "envelope.{field} returned a function, not a number: a curve \
         constructor needs its (), as in synth.slope.exponential(2)"
    )]
    CurveUncalled { field: &'static str },

    #[error("envelope.{field} must return a number from 0 to 1, got {got}")]
    CurveType {
        field: &'static str,
        got: &'static str,
    },

    #[error("envelope.{field} returned {value} at t = {t}; must be a number from 0 to 1")]
    CurveNotFinite {
        field: &'static str,
        value: f64,
        t: f64,
    },

    #[error("step {step} has a note but no instrument is in effect")]
    NoInstrument { step: usize },

    #[error("step {step} names instrument `{name}`, which is not defined")]
    UndefinedInstrument { step: usize, name: String },

    /// Lua's own message already says what failed, so it is shown as is.
    #[error(transparent)]
    Lua(#[from] mlua::Error),
}

/// `an envelope`, `a keyboard`.
fn article(kind: &str) -> String {
    match kind.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => format!("an {kind}"),
        _ => format!("a {kind}"),
    }
}

/// How to build a value of `kind`, for an error about an untagged table.
fn constructor(kind: &str) -> String {
    match kind {
        // filters have one constructor per type
        "filter" => "synth.filter.lowpass({…}) or another synth.filter constructor".to_string(),
        other => format!("synth.{other}({{…}})"),
    }
}

/// Exponent form only where plain digits would run on.
fn hz_shown(hz: f64) -> String {
    if hz.abs() >= 1e9 {
        format!("{hz:e}")
    } else {
        format!("{hz}")
    }
}
