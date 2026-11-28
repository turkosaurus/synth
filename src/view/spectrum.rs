//! Spectrum: amplitude per bin, drawn as bars on a log frequency axis from
//! one bin to nyquist.
//!
//! Only `core`, `alloc`, and ratatui's no_std widgets, so it can draw on an
//! embedded display. The transform that makes the bins stays with the
//! runtime, so an embedded build can swap it without touching this.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Sparkline, Widget};

use crate::approx;

/// Bar heights: fine enough that a bar moves smoothly, and absolute, so a
/// given height is always the same level.
const STEPS: u64 = 100;

/// Amplitude per bin, DC to nyquist, of a transform of `size` samples at
/// `sample_rate`, drawn one bar per column.
pub(crate) struct Spectrum<'a> {
    pub(crate) amplitudes: &'a [f32],
    pub(crate) size: usize,
    pub(crate) sample_rate: f32,
    /// the quietest level drawn, in dB below full scale
    pub(crate) floor_db: f32,
}

impl Spectrum<'_> {
    /// Labels for the axis's ends: one bin, and nyquist.
    pub(crate) fn axis(&self) -> (String, String) {
        let bin = self.sample_rate / self.size as f32;
        (hz(bin), hz(self.sample_rate / 2.0))
    }
}

impl Widget for Spectrum<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let columns = columns(self.amplitudes, usize::from(area.width), self.floor_db);
        Sparkline::default()
            .data(
                columns
                    .into_iter()
                    .map(|column| (column * STEPS as f32 + 0.5) as u64),
            )
            .max(STEPS)
            .style(Style::new().fg(Color::Cyan))
            .render(area, buf);
    }
}

/// `width` columns on a log frequency axis from one bin up to nyquist,
/// each the loudest bin it covers as a 0.0..1.0 position on a dB scale.
/// Starting at one bin rather than DC keeps every column a frequency the
/// transform can resolve.
fn columns(amplitudes: &[f32], width: usize, floor_db: f32) -> Vec<f32> {
    let nyquist = amplitudes.len().saturating_sub(1);
    if nyquist == 0 {
        return alloc::vec![0.0; width];
    }
    let octaves = approx::log2(nyquist as f32);
    // column edges in bins: 1 at the left, nyquist at the right
    let edge = |at: f32| approx::exp2(octaves * at / width as f32);
    (0..width)
        .map(|column| {
            let first = approx::ceil(edge(column as f32)) as usize;
            // the last column takes nyquist, which no edge passes
            let last = if column + 1 == width {
                nyquist
            } else {
                (approx::ceil(edge(column as f32 + 1.0)) as usize).saturating_sub(1)
            };
            // low columns are narrower than a bin, so they repeat the
            // bin nearest their centre
            let loudest = if first <= last {
                amplitudes[first..=last].iter().copied().fold(0.0, f32::max)
            } else {
                amplitudes[(edge(column as f32 + 0.5) + 0.5) as usize]
            };
            scale(loudest, floor_db)
        })
        .collect()
}

/// Amplitude to a 0.0..1.0 position, with `floor_db` (below 0) at the
/// bottom and full scale at the top.
fn scale(amplitude: f32, floor_db: f32) -> f32 {
    if amplitude < f32::MIN_POSITIVE {
        return 0.0;
    }
    let db = 20.0 * core::f32::consts::LOG10_2 * approx::log2(amplitude);
    ((db - floor_db) / -floor_db).clamp(0.0, 1.0)
}

fn hz(hz: f32) -> String {
    if hz < 1000.0 {
        format!("{hz:.1} Hz")
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

#[cfg(test)]
mod tests;
