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
