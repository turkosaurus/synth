//! Live spectrum: the newest samples of a frame source, Hann windowed and
//! transformed on the UI thread, read out on a log frequency axis.
//! Pure arithmetic, so it is tested without a terminal or a device, the way
//! `meter` is.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{FftError, RealFftPlanner, RealToComplex};

use super::meter;

/// The newest `size` samples, and their amplitude per bin once
/// [`Spectrum::update`] has transformed them.
pub(crate) struct Spectrum {
    size: usize,
    /// a circular buffer whose oldest sample is at `next`
    history: Vec<f32>,
    next: usize,
    window: Vec<f32>,
    /// `2 / sum(window)`, so a full-scale sine on a bin reads 1.0
    gain: f32,
    fft: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// amplitude per bin, DC to nyquist
    amplitudes: Vec<f32>,
}

impl Spectrum {
    /// Transforms of `size` samples, which must be even so the last bin is
    /// nyquist.
    pub(crate) fn new(size: usize) -> Self {
        assert!(
            size >= 2 && size.is_multiple_of(2),
            "odd spectrum size {size}"
        );
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);
        // periodic rather than symmetric: the frame repeats every `size`
        // samples as far as the transform is concerned
        let window: Vec<f32> = (0..size)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
            .collect();
        let gain = 2.0 / window.iter().sum::<f32>();
        Self {
            size,
            history: vec![0.0; size],
            next: 0,
            window,
            gain,
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            amplitudes: vec![0.0; size / 2 + 1],
            fft,
        }
    }

    /// The width of one bin at `sample_rate`, and the lowest frequency the
    /// axis shows.
    pub(crate) fn bin_hz(&self, sample_rate: f32) -> f32 {
        sample_rate / self.size as f32
    }

    /// Fold in one sample; only the newest `size` are kept.
    pub(crate) fn push(&mut self, sample: f32) {
        self.history[self.next] = sample;
        self.next = (self.next + 1) % self.size;
    }

    /// Forget what was read, so a new source does not show the old one's
    /// spectrum.
    pub(crate) fn clear(&mut self) {
        self.history.fill(0.0);
        self.next = 0;
    }

    /// Transform the samples held now.
    pub(crate) fn update(&mut self) -> Result<(), FftError> {
        let (old, new) = self.history.split_at(self.next);
        let ordered = new.iter().chain(old);
        for ((input, sample), weight) in self.input.iter_mut().zip(ordered).zip(&self.window) {
            *input = sample * weight;
        }
        self.fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)?;

        let last = self.output.len() - 1;
        for (bin, (amplitude, value)) in self.amplitudes.iter_mut().zip(&self.output).enumerate() {
            // DC and nyquist have no mirror image to fold back in
            let fold = if bin == 0 || bin == last { 0.5 } else { 1.0 };
            *amplitude = value.norm() * self.gain * fold;
        }
        Ok(())
    }

    /// `width` columns on a log frequency axis from one bin up to nyquist,
    /// each the loudest bin it covers as a 0.0..1.0 [`meter::scale`]
    /// position. Starting at one bin rather than DC keeps every column a
    /// frequency the transform can resolve.
    pub(crate) fn columns(&self, width: usize, floor_db: f32) -> Vec<f32> {
        let nyquist = self.size / 2;
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
                    self.amplitudes[first..=last]
                        .iter()
                        .copied()
                        .fold(0.0, f32::max)
                } else {
                    self.amplitudes[(low * high).sqrt().round() as usize]
                };
                meter::scale(loudest, floor_db)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Spectrum;

    const SIZE: usize = 4096;
    const FLOOR_DB: f32 = -60.0;

    fn sine(bin: f32, amplitude: f32) -> impl Iterator<Item = f32> {
        (0..SIZE)
            .map(move |i| amplitude * (std::f32::consts::TAU * bin * i as f32 / SIZE as f32).sin())
    }

    fn transformed(samples: impl IntoIterator<Item = f32>) -> Spectrum {
        let mut spectrum = Spectrum::new(SIZE);
        samples.into_iter().for_each(|sample| spectrum.push(sample));
        spectrum.update().unwrap();
        spectrum
    }

    #[track_caller]
    fn near(got: f32, want: f32) {
        assert!((got - want).abs() < 0.01, "got {got}, want {want}");
    }

    #[test]
    fn a_full_scale_sine_on_a_bin_reads_full_scale() {
        let spectrum = transformed(sine(100.0, 1.0));
        near(spectrum.amplitudes[100], 1.0);
    }

    #[test]
    fn half_amplitude_reads_half() {
        let spectrum = transformed(sine(100.0, 0.5));
        near(spectrum.amplitudes[100], 0.5);
    }

    #[test]
    fn the_window_keeps_leakage_off_distant_bins() {
        // halfway between bins, the worst case for leakage
        let spectrum = transformed(sine(100.5, 1.0));
        assert!(
            spectrum.amplitudes[120] < 0.001,
            "{}",
            spectrum.amplitudes[120]
        );
    }

    #[test]
    fn silence_reads_empty() {
        let spectrum = transformed(std::iter::empty());
        let columns = spectrum.columns(80, FLOOR_DB);
        assert!(columns.iter().all(|&column| column == 0.0), "{columns:?}");
    }

    #[test]
    fn only_the_newest_samples_are_transformed() {
        let mut spectrum = Spectrum::new(SIZE);
        sine(50.0, 1.0).for_each(|sample| spectrum.push(sample));
        sine(400.0, 1.0).for_each(|sample| spectrum.push(sample));
        spectrum.update().unwrap();
        near(spectrum.amplitudes[400], 1.0);
        assert!(
            spectrum.amplitudes[50] < 0.001,
            "{}",
            spectrum.amplitudes[50]
        );
    }

    #[test]
    fn the_axis_is_logarithmic() {
        // one bin is 2^0 and nyquist is 2^11, so with 11 columns each
        // column is one octave: bin 2^k lands in column k
        let width = 11;
        for octave in 0..width {
            let bin = 2f32.powi(octave as i32) * 1.4;
            let columns = transformed(sine(bin.round(), 1.0)).columns(width, FLOOR_DB);
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
        let mut spectrum = Spectrum::new(SIZE);
        spectrum.amplitudes[SIZE / 2] = 1.0;
        let columns = spectrum.columns(40, FLOOR_DB);
        assert_eq!(columns.last(), Some(&1.0));
    }

    #[test]
    fn every_column_draws_when_columns_outnumber_low_bins() {
        let mut spectrum = Spectrum::new(SIZE);
        spectrum.amplitudes.fill(1.0);
        let columns = spectrum.columns(400, FLOOR_DB);
        assert!(columns.iter().all(|&column| column == 1.0), "{columns:?}");
    }

    #[test]
    fn a_smaller_size_still_reads_full_scale() {
        let mut spectrum = Spectrum::new(256);
        (0..256)
            .map(|i| (std::f32::consts::TAU * 10.0 * i as f32 / 256.0).sin())
            .for_each(|sample| spectrum.push(sample));
        spectrum.update().unwrap();
        near(spectrum.amplitudes[10], 1.0);
        near(spectrum.bin_hz(48_000.0), 187.5);
    }

    #[test]
    fn clear_forgets_the_old_source() {
        let mut spectrum = transformed(sine(100.0, 1.0));
        spectrum.clear();
        spectrum.update().unwrap();
        let columns = spectrum.columns(80, FLOOR_DB);
        assert!(columns.iter().all(|&column| column == 0.0), "{columns:?}");
    }
}
