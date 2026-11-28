//! The spectrum's transform: the newest samples of a frame source, Hann
//! windowed and transformed on the UI thread. It needs std, as `realfft`
//! does, so it stays out of the widget that draws its bins.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{FftError, RealFftPlanner, RealToComplex};

/// Amplitude per bin of the newest `size` samples handed to
/// [`Transform::update`].
pub(crate) struct Transform {
    size: usize,
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
            window,
            gain,
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            amplitudes: vec![0.0; size / 2 + 1],
            fft,
        }
    }

    /// Transform the newest `size` of `recent`, oldest first; fewer are
    /// led by silence.
    pub(crate) fn update(&mut self, recent: &[f32]) -> Result<(), FftError> {
        let newest = &recent[recent.len().saturating_sub(self.size)..];
        let silence = self.size - newest.len();
        self.input[..silence].fill(0.0);
        for ((input, sample), weight) in self.input[silence..]
            .iter_mut()
            .zip(newest)
            .zip(&self.window[silence..])
        {
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
mod tests;
