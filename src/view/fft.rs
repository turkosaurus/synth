//! The spectrum's transform: the newest samples of a frame source, Hann
//! windowed and transformed on the UI thread. It needs std, as `realfft`
//! does, so it stays out of the widget that draws its bins.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{FftError, RealFftPlanner, RealToComplex};

/// The newest `size` samples, and their amplitude per bin once
/// [`Transform::update`] has transformed them.
pub(crate) struct Transform {
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

impl Transform {
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

    pub(crate) fn size(&self) -> usize {
        self.size
    }

    /// Amplitude per bin, DC to nyquist, as of the last [`Transform::update`].
    pub(crate) fn amplitudes(&self) -> &[f32] {
        &self.amplitudes
    }
}

#[cfg(test)]
mod tests {
    use super::Transform;

    const SIZE: usize = 4096;

    fn sine(bin: f32, amplitude: f32) -> impl Iterator<Item = f32> {
        (0..SIZE)
            .map(move |i| amplitude * (std::f32::consts::TAU * bin * i as f32 / SIZE as f32).sin())
    }

    fn transformed(samples: impl IntoIterator<Item = f32>) -> Transform {
        let mut spectrum = Transform::new(SIZE);
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
    fn only_the_newest_samples_are_transformed() {
        let mut spectrum = Transform::new(SIZE);
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
    fn a_smaller_size_still_reads_full_scale() {
        let mut spectrum = Transform::new(256);
        (0..256)
            .map(|i| (std::f32::consts::TAU * 10.0 * i as f32 / 256.0).sin())
            .for_each(|sample| spectrum.push(sample));
        spectrum.update().unwrap();
        near(spectrum.amplitudes[10], 1.0);
    }

    #[test]
    fn clear_forgets_the_old_source() {
        let mut spectrum = transformed(sine(100.0, 1.0));
        spectrum.clear();
        spectrum.update().unwrap();
        assert!(spectrum.amplitudes.iter().all(|&a| a < 1e-6));
    }
}
