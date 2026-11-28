use super::Transform;

const SIZE: usize = 4096;

fn sine(bin: f32, amplitude: f32) -> impl Iterator<Item = f32> {
    (0..SIZE).map(move |i| amplitude * (std::f32::consts::TAU * bin * i as f32 / SIZE as f32).sin())
}

fn transformed(samples: impl IntoIterator<Item = f32>) -> Transform {
    let mut spectrum = Transform::new(SIZE);
    let recent: Vec<f32> = samples.into_iter().collect();
    spectrum.update(&recent).unwrap();
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
    let spectrum = transformed(sine(50.0, 1.0).chain(sine(400.0, 1.0)));
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
    let recent: Vec<f32> = (0..256)
        .map(|i| (std::f32::consts::TAU * 10.0 * i as f32 / 256.0).sin())
        .collect();
    spectrum.update(&recent).unwrap();
    near(spectrum.amplitudes[10], 1.0);
}

#[test]
fn fewer_samples_than_the_size_are_led_by_silence() {
    let mut spectrum = transformed(sine(100.0, 1.0));
    spectrum.update(&[]).unwrap();
    assert!(spectrum.amplitudes.iter().all(|&a| a < 1e-6));
    spectrum.update(&[0.0; 10]).unwrap();
    assert!(spectrum.amplitudes.iter().all(|&a| a < 1e-6));
}

#[test]
fn a_frame_never_allocates_after_setup() {
    use crate::audio::output_tee;
    use crate::session::{Pick, Selection, Sources, Stream};

    let (mut tx, tee) = output_tee(64, 2);
    let selection = Selection {
        stream: 0,
        pick: Pick::Mix,
    };
    let mut sources = Sources::new(vec![Stream::new("output", 48_000.0, tee)], selection);
    sources.keep(256);
    let mut transform = Transform::new(256);
    let mut frame = |tx: &mut rtrb::Producer<f32>, sources: &mut Sources| {
        while tx.push(0.5).is_ok() {}
        sources.read();
        transform.update(sources.recent()).unwrap();
    };
    frame(&mut tx, &mut sources);

    // 64 slots, 32 stereo frames a read: eight more reads overfill
    // the window, so it drops its oldest samples
    let info = allocation_counter::measure(|| {
        for _ in 0..12 {
            frame(&mut tx, &mut sources);
        }
    });
    assert_eq!(info.count_total, 0);
    assert_eq!(sources.recent().len(), 256);
}
