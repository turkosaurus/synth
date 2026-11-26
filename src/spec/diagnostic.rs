/// What a diagnostic is about. A check names the kind; deciding how serious
/// each kind is stays out of the checks, so a stricter policy later changes
/// one place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// a baked shape that will sound wrong: flattened, buzzing, off-center,
    /// or clicking
    Shape,
    /// a value outside the range the engine plays, used anyway or clamped
    Range,
    /// a field set where it has no effect
    Unused,
    /// what a patch passed to `print`
    Print,
}

/// How serious a diagnostic is. Only [`DiagnosticKind::severity`] decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Severity {
    Info,
    Warn,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
        }
    }
}

impl DiagnosticKind {
    fn label(self) -> &'static str {
        match self {
            Self::Shape => "shape",
            Self::Range => "range",
            Self::Unused => "unused",
            Self::Print => "print",
        }
    }

    pub fn severity(self) -> Severity {
        match self {
            Self::Shape | Self::Range | Self::Unused => Severity::Warn,
            Self::Print => Severity::Info,
        }
    }
}

/// Something the loader noticed about a patch that does not stop it loading.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    /// `file:line` of the constructor that built the value it is about,
    /// filled in once the patch's structure is known
    pub at: Option<String>,
}

impl Diagnostic {
    pub(crate) fn new(kind: DiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            at: None,
        }
    }

    pub fn severity(&self) -> Severity {
        self.kind.severity()
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.at {
            Some(at) => write!(f, "{}  ({at}, {})", self.message, self.kind.label()),
            None => write!(f, "{}  ({})", self.message, self.kind.label()),
        }
    }
}
