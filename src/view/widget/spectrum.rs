//! The spectrum's bars: amplitude per bin, on a log frequency axis from one
//! bin to nyquist. The transform that makes the bins is the runtime's.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Sparkline, Widget};

use crate::view::meter;

/// Amplitude per bin, DC to nyquist, of a transform of `size` samples at
/// `sample_rate`, drawn one bar per column.
pub(crate) struct Spectrum<'a> {
    pub(crate) amplitudes: &'a [f32],
    pub(crate) size: usize,
    pub(crate) sample_rate: f32,
    /// the quietest level drawn, in dB below full scale
    pub(crate) floor_db: f32,
    pub(crate) block: Block<'a>,
}

impl Widget for Spectrum<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let bin = self.sample_rate / self.size as f32;
        let block = self
            .block
            .title_bottom(Line::from(format!(" {} ", hz(bin))).left_aligned())
            .title_bottom(Line::from(format!(" {} ", hz(self.sample_rate / 2.0))).right_aligned());
        let inner = block.inner(area);
        block.render(area, buf);

        let columns = columns(self.amplitudes, usize::from(inner.width), self.floor_db);
        Sparkline::default()
            .data(
                columns
                    .into_iter()
                    .map(|column| (column * meter::STEPS as f32).round() as u64),
            )
            // absolute, as the activity graph is: a given height is
            // always the same level
            .max(meter::STEPS)
            .style(Style::new().fg(Color::Cyan))
            .render(inner, buf);
    }
}

/// `width` columns on a log frequency axis from one bin up to nyquist,
/// each the loudest bin it covers as a 0.0..1.0 [`meter::scale`]
/// position. Starting at one bin rather than DC keeps every column a
/// frequency the transform can resolve.
fn columns(amplitudes: &[f32], width: usize, floor_db: f32) -> Vec<f32> {
    let nyquist = amplitudes.len().saturating_sub(1);
    if nyquist == 0 {
        return vec![0.0; width];
    }
    let top = nyquist as f32;
    // column edges in bins: 1 at the left, nyquist at the right
    let edge = |column: usize| top.powf(column as f32 / width as f32);
    (0..width)
        .map(|column| {
            let (low, high) = (edge(column), edge(column + 1));
            let first = low.ceil() as usize;
            // the last column takes nyquist, which no edge passes
            let last = if column + 1 == width {
                nyquist
            } else {
                (high.ceil() as usize).saturating_sub(1)
            };
            // low columns are narrower than a bin, so they repeat the
            // bin nearest their centre
            let loudest = if first <= last {
                amplitudes[first..=last].iter().copied().fold(0.0, f32::max)
            } else {
                amplitudes[(low * high).sqrt().round() as usize]
            };
            meter::scale(loudest, floor_db)
        })
        .collect()
}

fn hz(hz: f32) -> String {
    if hz < 1000.0 {
        format!("{hz:.1} Hz")
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = 4096;
    const FLOOR_DB: f32 = -60.0;

    /// Bins of a `SIZE` transform, silent but for `loud`.
    fn bins(loud: &[usize]) -> Vec<f32> {
        let mut amplitudes = vec![0.0; SIZE / 2 + 1];
        for &bin in loud {
            amplitudes[bin] = 1.0;
        }
        amplitudes
    }

    #[test]
    fn silence_reads_empty() {
        let columns = columns(&bins(&[]), 80, FLOOR_DB);
        assert!(columns.iter().all(|&column| column == 0.0), "{columns:?}");
    }

    #[test]
    fn the_axis_is_logarithmic() {
        // one bin is 2^0 and nyquist is 2^11, so with 11 columns each
        // column is one octave: bin 2^k lands in column k
        let width = 11;
        for octave in 0..width {
            let bin = (2f32.powi(octave as i32) * 1.4).round() as usize;
            let columns = columns(&bins(&[bin]), width, FLOOR_DB);
            let loudest = columns
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(column, _)| column);
            assert_eq!(loudest, Some(octave), "bin {bin}: {columns:?}");
        }
    }

    #[test]
    fn the_last_column_reaches_nyquist() {
        let columns = columns(&bins(&[SIZE / 2]), 40, FLOOR_DB);
        assert_eq!(columns.last(), Some(&1.0));
    }

    #[test]
    fn every_column_draws_when_columns_outnumber_low_bins() {
        let columns = columns(&[1.0; SIZE / 2 + 1], 400, FLOOR_DB);
        assert!(columns.iter().all(|&column| column == 1.0), "{columns:?}");
    }

    #[test]
    fn the_axis_is_labelled_with_one_bin_and_nyquist() {
        let area = Rect::new(0, 0, 40, 6);
        let mut buffer = Buffer::empty(area);
        let amplitudes = bins(&[100]);
        Spectrum {
            amplitudes: &amplitudes,
            size: SIZE,
            sample_rate: 48_000.0,
            floor_db: FLOOR_DB,
            block: Block::bordered(),
        }
        .render(area, &mut buffer);
        let bottom: String = (0..area.width)
            .map(|x| buffer[(x, area.height - 1)].symbol().to_string())
            .collect();
        assert!(bottom.contains("11.7 Hz"), "{bottom}");
        assert!(bottom.contains("24.0 kHz"), "{bottom}");
    }
}
