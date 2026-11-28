//! Oscilloscope state: the newest frames of a source, and the window of
//! them to draw.

use std::collections::VecDeque;

/// The newest samples of a source, and the pitch last sounded.
pub(crate) struct Scope {
    /// oldest first, at most `capacity` long
    history: VecDeque<f32>,
    capacity: usize,
    /// cycles of the pitch one window spans
    cycles: f32,
    /// kept after the key lifts, so a release tail stays triggered
    hz: Option<f32>,
}

impl Scope {
    /// A scope keeping the newest `capacity` samples, drawing `cycles` of
    /// the pitch. A window needs one more cycle than it draws to find its
    /// trigger in.
    pub(crate) fn new(capacity: usize, cycles: f32) -> Self {
        Self {
            history: VecDeque::with_capacity(capacity),
            capacity,
            cycles,
            hz: None,
        }
    }

    /// Keep `sample`, dropping the oldest once full.
    pub(crate) fn push(&mut self, sample: f32) {
        if self.history.len() == self.capacity {
            self.history.pop_front();
        }
        self.history.push_back(sample);
    }

    /// Forget what was read, so a new source does not draw the old one's
    /// tail.
    pub(crate) fn clear(&mut self) {
        self.history.clear();
    }

    /// The pitch to trigger on; `None` keeps the last one.
    pub(crate) fn tune(&mut self, hz: Option<f32>) {
        if let Some(hz) = hz.filter(|hz| *hz > 0.0) {
            self.hz = Some(hz);
        }
    }

    /// `cycles` of the pitch last tuned to, starting on the newest rising
    /// zero crossing that leaves room for all of them; empty before any
    /// pitch is known.
    pub(crate) fn window(&mut self, sample_rate: f32) -> &[f32] {
        let history = self.history.make_contiguous();
        let Some(hz) = self.hz else {
            return &[];
        };
        let len = (self.cycles * sample_rate / hz).round() as usize;
        window(history, len)
    }
}

/// The last `len` samples of `samples` that start on a rising zero
/// crossing, or simply the last `len` if none has room after it, so the
/// wave at least shows while it jitters.
fn window(samples: &[f32], len: usize) -> &[f32] {
    let len = len.min(samples.len());
    let last = samples.len() - len;
    let start = (1..=last)
        .rev()
        .find(|&i| samples[i - 1] < 0.0 && samples[i] >= 0.0)
        .unwrap_or(last);
    &samples[start..start + len]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::output_tee;
    use crate::session::{Pick, Selection, Sources, Stream};

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

    #[test]
    fn read_keeps_the_newest_samples_of_the_source() {
        let (mut tx, tee) = output_tee(8, 2);
        for frame in 0..6 {
            tx.push(frame as f32).unwrap();
            tx.push(-1.0).unwrap();
        }
        let selection = Selection {
            stream: 0,
            pick: Pick::Channel(0),
        };
        let mut sources = Sources::new(vec![Stream::new("output", 48_000.0, tee)], selection);
        let mut scope = Scope::new(4, 2.0);
        sources.read(|sample| scope.push(sample));
        assert_eq!(scope.history, [2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn the_window_spans_the_configured_cycles() {
        let mut scope = Scope::new(400, 3.0);
        scope.history.extend(sine(100.0, 4800.0, 0.0, 400));
        scope.tune(Some(100.0));
        assert_eq!(scope.window(4800.0).len(), 144);
    }

    #[test]
    fn a_lifted_key_keeps_the_last_pitch() {
        let mut scope = Scope::new(16, 2.0);
        assert!(scope.window(48_000.0).is_empty());
        scope.tune(Some(440.0));
        scope.tune(None);
        assert_eq!(scope.hz, Some(440.0));
    }
}
