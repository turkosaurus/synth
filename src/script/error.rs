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
