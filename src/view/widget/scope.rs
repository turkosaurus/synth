//! The scope's trace: a window of samples on a fixed -1..1 axis.

use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{self, Canvas, Line as Segment};

/// `wave` across the whole width on a fixed -1..1 axis, so clipping reads as
/// a flat top rather than being scaled away. Braille gives each cell 2x4
/// dots, and joining samples keeps a high pitch from drawing as dots.
pub(crate) fn oscilloscope(wave: &[f32]) -> Canvas<'_, impl Fn(&mut canvas::Context) + '_> {
    Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, wave.len().saturating_sub(1).max(1) as f64])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| {
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
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    use super::oscilloscope;

    fn top_row(wave: &[f32]) -> String {
        let area = Rect::new(0, 0, 24, 16);
        let mut buffer = Buffer::empty(area);
        oscilloscope(wave).render(area, &mut buffer);
        (0..area.width)
            .map(|x| buffer[(x, 0)].symbol().to_string())
            .collect()
    }

    #[test]
    fn clipping_draws_a_flat_top() {
        let sine = |gain: f32| -> Vec<f32> {
            (0..96)
                .map(|i| (gain * (core::f32::consts::TAU * i as f32 / 96.0).sin()).clamp(-1.0, 1.0))
                .collect()
        };
        let lit = |row: String| row.chars().filter(|c| *c != ' ' && *c != '⠀').count();
        let (clean, clipped) = (top_row(&sine(1.0)), top_row(&sine(4.0)));
        assert!(lit(clipped) > 2 * lit(clean));
    }
}
