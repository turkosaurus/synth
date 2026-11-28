use super::*;

fn sine(hz: f32, sample_rate: f32, phase: f32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| (std::f32::consts::TAU * (hz * i as f32 / sample_rate + phase)).sin())
        .collect()
}

#[test]
fn the_window_starts_on_a_rising_zero_crossing() {
    let samples = sine(100.0, 4800.0, 0.3, 400);
    let shown = window(&samples, 96);
    assert_eq!(shown.len(), 96);
    assert!(shown[0] >= 0.0 && shown[0] < 0.2, "{}", shown[0]);
    assert!(shown[1] > shown[0]);
}

#[test]
fn the_window_holds_still_whatever_the_phase_read_at() {
    let first = window(&sine(100.0, 4800.0, 0.1, 400), 96).to_vec();
    for phase in [0.2, 0.45, 0.7, 0.95] {
        let next = window(&sine(100.0, 4800.0, phase, 400), 96).to_vec();
        for (a, b) in first.iter().zip(&next) {
            assert!((a - b).abs() < 0.15, "{a} {b} at phase {phase}");
        }
    }
}

#[test]
fn a_wave_with_no_crossing_shows_its_newest_samples() {
    let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
    assert_eq!(window(&samples, 3), &[7.0, 8.0, 9.0]);
    assert_eq!(window(&samples, 20).len(), 10);
}

fn top_row(wave: &[f32]) -> String {
    let area = Rect::new(0, 0, 24, 16);
    let mut buffer = Buffer::empty(area);
    Oscilloscope { wave }.render(area, &mut buffer);
    (0..area.width)
        .map(|x| buffer[(x, 0)].symbol().to_string())
        .collect()
}

#[test]
fn clipping_draws_a_flat_top() {
    let sine = |gain: f32| -> Vec<f32> {
        (0..96)
            .map(|i| (gain * (std::f32::consts::TAU * i as f32 / 96.0).sin()).clamp(-1.0, 1.0))
            .collect()
    };
    let lit = |row: String| row.chars().filter(|c| *c != ' ' && *c != '⠀').count();
    let (clean, clipped) = (top_row(&sine(1.0)), top_row(&sine(4.0)));
    assert!(lit(clipped) > 2 * lit(clean));
}

#[test]
fn the_window_spans_the_configured_cycles() {
    let mut scope = Scope::new(3.0);
    scope.tune(Some(100.0));
    let history = sine(100.0, 4800.0, 0.0, 400);
    assert_eq!(scope.window(&history, 4800.0).len(), 144);
}

#[test]
fn a_lifted_key_keeps_the_last_pitch() {
    let mut scope = Scope::new(2.0);
    assert!(scope.window(&[0.0; 16], 48_000.0).is_empty());
    scope.tune(Some(440.0));
    scope.tune(None);
    assert_eq!(scope.hz, Some(440.0));
}
