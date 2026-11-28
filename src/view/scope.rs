//! Oscilloscope: the newest samples of a source, the window of them held
//! on a trigger, and the widget that draws it.
//!
//! Only `core`, `alloc`, and ratatui's no_std widgets, so it can draw on an
//! embedded display. Reading the source stays with the runtime.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Line as Segment};
use ratatui::widgets::Widget;

/// The pitch last sounded, and how much of it a window spans.
pub(crate) struct Scope {
    /// cycles of the pitch one window spans
    cycles: f32,
    /// kept after the key lifts, so a release tail stays triggered
    hz: Option<f32>,
}

impl Scope {
    /// A scope drawing `cycles` of the pitch. A window needs one more
    /// cycle of history than it draws to find its trigger in.
    pub(crate) fn new(cycles: f32) -> Self {
        Self { cycles, hz: None }
    }

    /// The pitch to trigger on; `None` keeps the last one.
    pub(crate) fn tune(&mut self, hz: Option<f32>) {
        if let Some(hz) = hz.filter(|hz| *hz > 0.0) {
            self.hz = Some(hz);
        }
    }

    /// `cycles` of the pitch last tuned to from `history`, oldest first,
    /// starting on the newest rising zero crossing that leaves room for all
    /// of them; empty before any pitch is known.
    pub(crate) fn window<'a>(&self, history: &'a [f32], sample_rate: f32) -> &'a [f32] {
        let Some(hz) = self.hz else {
            return &[];
        };
        // rounded by hand: f32::round is not in core
        let len = (self.cycles * sample_rate / hz + 0.5) as usize;
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

/// `wave` across the whole area on a fixed -1..1 axis, so clipping reads as
/// a flat top rather than being scaled away. Braille gives each cell 2x4
/// dots, and joining samples keeps a high pitch from drawing as dots.
pub(crate) struct Oscilloscope<'a> {
    /// oldest first, already triggered
    pub(crate) wave: &'a [f32],
}

impl Widget for Oscilloscope<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let wave = self.wave;
        Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([0.0, wave.len().saturating_sub(1).max(1) as f64])
            .y_bounds([-1.0, 1.0])
            .paint(|ctx| {
                ctx.draw(&Segment::new(
                    0.0,
                    0.0,
                    wave.len() as f64,
                    0.0,
                    Color::DarkGray,
                ));
                ctx.layer();
                for (x, pair) in wave.windows(2).enumerate() {
                    ctx.draw(&Segment::new(
                        x as f64,
                        f64::from(pair[0]),
                        x as f64 + 1.0,
                        f64::from(pair[1]),
                        Color::Green,
                    ));
                }
            })
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests;
