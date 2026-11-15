use super::{Filter, FilterReport, Keymap, Timeline};

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
}

impl Patch {
    /// Neither an instrument nor the filter raised a warning, so the patch is safe to play as written.
    pub fn clean(&self) -> bool {
        self.reports
            .iter()
            .all(|(_, report)| report.warnings.is_empty())
            && self
                .filter_report
                .as_ref()
                .is_none_or(|report| report.warnings.is_empty())
    }
}

/// Measurements and warnings from a baked patch. Pure data, so the caller
/// decides whether to log them, print them, or fail on them.
#[derive(Debug)]
pub struct Report {
    pub(crate) peak: f32,
    pub(crate) dc: f32,
    pub(crate) seam: f32,
    pub(crate) clamped: usize,
    pub(crate) attack: usize,
    pub(crate) decay: usize,
    pub(crate) release: usize,
    pub(crate) sustain_level: f32,
    pub warnings: Vec<String>,
}
