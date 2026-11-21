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

    /// Lua's own message already says what failed, so it is shown as is.
    #[error(transparent)]
    Lua(#[from] mlua::Error),
}
