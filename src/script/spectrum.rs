//! Harmonic content of a baked cycle.

/// The loudest harmonic in each octave of `cycle`, as an amplitude where a
/// full-scale sine reads 1.0: harmonic 1, then 2–3, 4–7, and so on, the
/// last octave running on to the table's nyquist, `cycle.len() / 2`.
///
/// A cycle is exactly one period, so bin `k` of its transform is harmonic
/// `k` with no leakage between them, and no window is applied: a Hann
/// window would spread each harmonic into its neighbours.
pub(super) fn octaves(cycle: &[f32]) -> Vec<f32> {
    let len = cycle.len();
    if len < 2 {
        return Vec::new();
    }
    let fft = realfft::RealFftPlanner::<f32>::new().plan_fft_forward(len);
    let mut input = cycle.to_vec();
    let mut bins = fft.make_output_vec();
    // the buffers come from this plan, so their lengths always match
    if fft.process(&mut input, &mut bins).is_err() {
        return Vec::new();
    }
    let nyquist = len / 2;
    let mut bands = Vec::new();
    for (harmonic, bin) in bins.iter().enumerate().take(nyquist + 1).skip(1) {
        // a real cycle splits each harmonic between bins k and len - k,
        // except at nyquist, which has no mirror
        let scale = if 2 * harmonic == len { 1.0 } else { 2.0 };
        let amplitude = scale * bin.norm() / len as f32;
        // nyquist would start an octave of one bin
        let band = harmonic.min(nyquist - 1).max(1).ilog2() as usize;
        if band == bands.len() {
            bands.push(0.0f32);
        }
        bands[band] = bands[band].max(amplitude);
    }
    bands
}

#[cfg(test)]
mod tests {
    use super::octaves;

    const LEN: usize = 2048;

    fn cycle(wave: impl Fn(f64) -> f64) -> Vec<f32> {
        (0..LEN)
            .map(|i| wave(i as f64 / LEN as f64) as f32)
            .collect()
    }

    fn db(amplitude: f32) -> f32 {
        20.0 * amplitude.log10()
    }

    #[test]
    fn a_band_per_octave_up_to_nyquist() {
        let bands = octaves(&cycle(|_| 0.0));
        // 1, 2–3, ..., 256–511, 512–1024
        assert_eq!(bands.len(), 10);
    }

    #[test]
    fn a_sine_is_its_fundamental_alone() {
        let tau = std::f64::consts::TAU;
        let bands = octaves(&cycle(|phase| (tau * phase).sin()));
        assert!((bands[0] - 1.0).abs() < 1e-4, "{bands:?}");
        assert!(bands[1..].iter().all(|&b| db(b) < -120.0), "{bands:?}");
    }

    #[test]
    fn a_triangle_falls_as_odd_harmonics_squared() {
        let bands = octaves(&cycle(|phase| 1.0 - 4.0 * (phase - 0.5).abs()));
        let fundamental = db(bands[0]);
        // the 3rd and 5th lead their octaves, at 1/9 and 1/25
        assert!((db(bands[1]) - fundamental + 19.08).abs() < 0.05);
        assert!((db(bands[2]) - fundamental + 27.96).abs() < 0.05);
    }

    #[test]
    fn a_saw_falls_six_db_an_octave() {
        let bands = octaves(&cycle(|phase| 1.0 - 2.0 * phase));
        let fundamental = db(bands[0]);
        for (octave, &band) in bands.iter().enumerate().take(6) {
            let want = -6.02 * octave as f32;
            assert!((db(band) - fundamental - want).abs() < 0.05, "{bands:?}");
        }
    }
}
