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
    let octaves = log2(nyquist as f32);
    // column edges in bins: 1 at the left, nyquist at the right
    let edge = |at: f32| exp2(octaves * at / width as f32);
    (0..width)
        .map(|column| {
            let first = ceil(edge(column as f32));
            // the last column takes nyquist, which no edge passes
            let last = if column + 1 == width {
                nyquist
            } else {
                ceil(edge(column as f32 + 1.0)).saturating_sub(1)
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
    let db = 20.0 * core::f32::consts::LOG10_2 * log2(amplitude);
    ((db - floor_db) / -floor_db).clamp(0.0, 1.0)
}

fn hz(hz: f32) -> String {
    if hz < 1000.0 {
        format!("{hz:.1} Hz")
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

// f32's log, exp, and rounding methods are std only, so these are by hand,
// accurate to about 1e-6, which is far finer than a column.

/// The smallest whole number at or above `x`, for `x` at least 0.
fn ceil(x: f32) -> usize {
    let whole = x as usize;
    if (whole as f32) < x {
        whole + 1
    } else {
        whole
    }
}

/// Base 2 log of a normal, positive `x`.
fn log2(x: f32) -> f32 {
    let bits = x.to_bits();
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    // the mantissa, in 1..2
    let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    // ln(m) = 2 atanh(s); s is at most 1/3, so five terms suffice
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let atanh = s * (1.0 + s2 * (1.0 / 3.0 + s2 * (1.0 / 5.0 + s2 * (1.0 / 7.0 + s2 / 9.0))));
    exponent as f32 + 2.0 * atanh * core::f32::consts::LOG2_E
}

/// 2 to the `y`, for `y` from 0 up to below 127.
fn exp2(y: f32) -> f32 {
    let whole = y as i32;
    let t = (y - whole as f32) * core::f32::consts::LN_2;
    // e^t for t below ln 2, by its series
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..=9 {
        term *= t / n as f32;
        sum += term;
    }
    sum * f32::from_bits(((whole + 127) as u32) << 23)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = 4096;
    const FLOOR_DB: f32 = -60.0;

    /// Bins of a `SIZE` transform, silent but for `loud`.
    fn bins(loud: &[usize]) -> Vec<f32> {
        let mut amplitudes = alloc::vec![0.0; SIZE / 2 + 1];
        for &bin in loud {
            amplitudes[bin] = 1.0;
        }
        amplitudes
    }

    #[track_caller]
    fn near(got: f32, want: f32) {
        assert!((got - want).abs() < 1e-5, "got {got}, want {want}");
    }

    #[test]
    fn the_math_matches_std() {
        for x in [1e-6, 0.001, 0.5, 1.0, 1.5, 3.0, 2048.0, 16384.0] {
            near(log2(x), x.log2());
        }
        for y in [0.0, 0.25, 1.0, 5.5, 10.999, 14.0] {
            near(exp2(y) / y.exp2(), 1.0);
        }
        for x in [0.0, 1.0, 1.0001, 2.5, 7.0] {
            assert_eq!(ceil(x), x.ceil() as usize, "{x}");
        }
    }

    #[test]
    fn full_scale_is_the_top_and_the_floor_the_bottom() {
        near(scale(1.0, FLOOR_DB), 1.0);
        near(scale(0.1, FLOOR_DB), 2.0 / 3.0);
        assert_eq!(scale(0.0001, FLOOR_DB), 0.0);
        assert_eq!(scale(0.0, FLOOR_DB), 0.0);
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
        let spectrum = Spectrum {
            amplitudes: &[],
            size: SIZE,
            sample_rate: 48_000.0,
            floor_db: FLOOR_DB,
        };
        assert_eq!(spectrum.axis(), ("11.7 Hz".into(), "24.0 kHz".into()));
    }

    #[test]
    fn a_loud_bin_draws_a_full_bar() {
        let area = Rect::new(0, 0, 20, 4);
        let mut buffer = Buffer::empty(area);
        let amplitudes = bins(&[SIZE / 2]);
        Spectrum {
            amplitudes: &amplitudes,
            size: SIZE,
            sample_rate: 48_000.0,
            floor_db: FLOOR_DB,
        }
        .render(area, &mut buffer);
        assert_eq!(buffer[(19, 0)].symbol(), "█");
        assert_eq!(buffer[(0, 0)].symbol(), " ");
    }
}
