use super::{Diagnostic, Filter, FilterReport, Keymap, Severity, Timeline};

/// A loaded file: a playable timeline plus one report per instrument in it.
pub struct Patch {
    pub timeline: Timeline,
    /// hz for every midi note, baked from Lua so temperament stays there
    pub freqs: Vec<f32>,
    pub(crate) keymap: Keymap,
    pub bpm: f32,
    pub(crate) division: f32,
    pub step_ms: f32,
    pub(crate) steps: usize,
    pub(crate) patterns: usize,
    pub(crate) chain: usize,
    pub(crate) loops: usize,
    pub reports: Vec<(String, Report)>,
    /// runs on the summed output of every voice
    pub filter: Option<Filter>,
    /// what `check` shows about `filter`
    pub filter_report: Option<FilterReport>,
    /// warnings about `freqs`, such as notes tuned above Nyquist
    pub tuning_diagnostics: Vec<Diagnostic>,
    /// what the patch passed to `print`, in call order
    pub printed: Vec<Diagnostic>,
    /// every table a constructor built, by where it sits in the patch
    pub(crate) structure: Vec<Node>,
}

/// One table a constructor built, located in the patch. The path is the
/// identity a later reload matches values by, so it never depends on where
/// a table sits in memory.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    /// where it sits, for example `instruments.tri.envelope`; empty for the root
    pub(crate) path: String,
    /// which constructor built it: `instrument`, `envelope`, ...
    pub(crate) kind: String,
    /// the `file:line` that called the constructor
    pub(crate) at: String,
}

impl Patch {
    /// Nothing raised a diagnostic at `warn` or above, so the patch is safe
    /// to play as written.
    pub fn clean(&self) -> bool {
        let reports = self
            .reports
            .iter()
            .flat_map(|(_, report)| &report.diagnostics);
        let filter = self
            .filter_report
            .iter()
            .flat_map(|report| &report.diagnostics);
        self.tuning_diagnostics
            .iter()
            .chain(reports)
            .chain(filter)
            .chain(&self.printed)
            .all(|diagnostic| diagnostic.severity() < Severity::Warn)
    }
}

/// Measurements and warnings from a baked patch. Pure data, so the caller
/// decides whether to log them, print them, or fail on them.
#[derive(Debug)]
pub struct Report {
    /// waves baked into the table; 1 for a plain waveform
    pub(crate) frames: usize,
    pub(crate) peak: f32,
    pub(crate) dc: f32,
    pub(crate) seam: f32,
    pub(crate) clamped: usize,
    pub(crate) attack: usize,
    pub(crate) decay: usize,
    pub(crate) release: usize,
    pub(crate) sustain_level: f32,
    pub diagnostics: Vec<Diagnostic>,
}
