//! The live scope and spectrum of the output tee, drawn by the `scope`
//! library.

use std::collections::VecDeque;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui_029::style::{Color as OldColor, Style as OldStyle};
use ratatui_029::symbols::Marker;
use ratatui_029::widgets::{Axis, Chart, Dataset, Widget as _};
use scope::display::oscilloscope::Oscilloscope;
use scope::display::spectroscope::Spectroscope;
use scope::display::{DataSet, Dimension, DisplayMode, GraphConfig};
use scope::input::{stream_to_matrix, Matrix};

use crate::audio::OutputTee;

/// Cycles of the sounding pitch the scope window spans.
const CYCLES: f32 = 2.0;
/// Frames per transform: 11.7 Hz bins at 48 kHz.
const FFT_FRAMES: usize = 4096;
/// The spectrum axis starts here; below it is one bin or less.
const LOW_HZ: f64 = 20.0;

pub(crate) struct Scopes {
    /// The newest output, interleaved as the tee delivers it.
    history: VecDeque<f32>,
    channels: usize,
    /// How many frames `history` holds.
    frames: usize,
    sample_rate: f32,
    /// The last pitch sounded, so a release tail stays triggered.
    hz: Option<f32>,
    cfg: GraphConfig,
    oscilloscope: Oscilloscope,
    spectroscope: Spectroscope,
    wave: Vec<DataSet>,
    spectrum: Vec<DataSet>,
}

impl Scopes {
    /// Keeps `seconds` of `tee`'s output, and never less than one
    /// transform's worth.
    pub(crate) fn new(tee: &OutputTee, sample_rate: f32, seconds: f32) -> Self {
        let frames = ((sample_rate * seconds) as usize).max(FFT_FRAMES);
        Self {
            // silence until the tee fills it, so every window is whole
            history: std::iter::repeat_n(0.0, frames * tee.channels).collect(),
            channels: tee.channels,
            frames,
            sample_rate,
            hz: None,
            cfg: GraphConfig {
                sampling_rate: sample_rate as u32,
                scale: 1.0,
                marker_type: Marker::Braille,
                palette: vec![OldColor::Green, OldColor::Cyan],
                labels_color: OldColor::DarkGray,
                axis_color: OldColor::DarkGray,
                ..GraphConfig::default()
            },
            oscilloscope: Oscilloscope {
                triggering: true,
                peaks: false,
                ..Oscilloscope::default()
            },
            spectroscope: Spectroscope {
                sampling_rate: sample_rate as u32,
                buffer_size: FFT_FRAMES as u32,
                average: 1,
                buf: Vec::new(),
                window: true,
                log_y: true,
                phase_diff: false,
            },
            wave: Vec::new(),
            spectrum: Vec::new(),
        }
    }

    /// Drain `tee`, follow `hz` while a note sounds, and redo both traces.
    pub(crate) fn update(&mut self, tee: &mut OutputTee, hz: Option<f32>) {
        if let Ok(chunk) = tee.samples.read_chunk(tee.samples.slots()) {
            self.history.extend(chunk);
        }
        let excess = self.history.len() - self.frames * self.channels;
        self.history.drain(..excess);

        self.hz = hz.or(self.hz);
        let capacity = self.frames;
        // a period past the window, so a rising crossing is always in reach
        let (window, tape) = match self.hz {
            Some(hz) => {
                let period = self.sample_rate / hz;
                let tape = ((CYCLES + 1.0) * period) as usize;
                ((CYCLES * period) as usize, tape.min(capacity))
            }
            None => (capacity, capacity),
        };
        self.oscilloscope.triggering = self.hz.is_some();
        self.cfg.samples = window.min(tape) as u32;
        self.wave = self.oscilloscope.process(&self.cfg, &self.newest(tape));
        self.spectrum = self
            .spectroscope
            .process(&self.cfg, &self.newest(FFT_FRAMES));
    }

    fn newest(&self, frames: usize) -> Matrix<f64> {
        let skip = self.history.len() - frames * self.channels;
        stream_to_matrix(self.history.range(skip..).copied(), self.channels, 1.0)
    }

    pub(crate) fn draw(&self, buf: &mut Buffer, wave: Rect, spectrum: Rect) {
        let refs = self.oscilloscope.references(&self.cfg);
        let scope = chart(
            &refs,
            &self.wave,
            self.oscilloscope.axis(&self.cfg, Dimension::X),
            self.oscilloscope.axis(&self.cfg, Dimension::Y),
        );
        render(scope, wave, buf);

        // the first reference is a level line whose length is
        // cfg.samples read as Hz, which means nothing here
        let refs: Vec<_> = self
            .spectroscope
            .references(&self.cfg)
            .into_iter()
            .skip(1)
            .collect();
        let nyquist = f64::from(self.sample_rate) / 2.0;
        let x = Axis::default()
            .style(OldStyle::new().fg(self.cfg.axis_color))
            .bounds([LOW_HZ.ln(), nyquist.ln()])
            .labels([
                format!("{LOW_HZ} Hz"),
                format!("{:.1} kHz", nyquist / 1000.0),
            ]);
        let spectro = chart(
            &refs,
            &self.spectrum,
            x,
            self.spectroscope.axis(&self.cfg, Dimension::Y),
        );
        render(spectro, spectrum, buf);
    }
}

/// References first, so the traces draw over them.
fn chart<'a>(refs: &'a [DataSet], sets: &'a [DataSet], x: Axis<'a>, y: Axis<'a>) -> Chart<'a> {
    let sets: Vec<Dataset> = refs.iter().chain(sets).map(Dataset::from).collect();
    // every channel carries the same mix, so a legend names nothing
    Chart::new(sets).x_axis(x).y_axis(y).legend_position(None)
}

/// Draws a ratatui 0.29 `chart` into a 0.30 `buf`, through a scratch
/// buffer whose symbols and foregrounds are copied across.
fn render(chart: Chart, area: Rect, buf: &mut Buffer) {
    let scratch = ratatui_029::layout::Rect::new(0, 0, area.width, area.height);
    let mut drawn = ratatui_029::buffer::Buffer::empty(scratch);
    chart.render(scratch, &mut drawn);
    let width = usize::from(area.width.max(1));
    for (i, cell) in drawn.content.iter().enumerate() {
        let (x, y) = ((i % width) as u16, (i / width) as u16);
        if let Some(to) = buf.cell_mut((area.x + x, area.y + y)) {
            to.set_symbol(cell.symbol()).set_fg(color(cell.fg));
        }
    }
}

/// Both versions print and parse colours the same way.
fn color(old: OldColor) -> Color {
    old.to_string().parse().unwrap_or(Color::Reset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio;

    const RATE: f32 = 48_000.0;
    const HZ: f32 = 480.0;

    fn sine(gain: f32, from: usize, frames: usize) -> impl Iterator<Item = f32> {
        (from..from + frames).map(move |i| {
            // off the sample grid, so no sample is a zero whose sign is
            // rounding noise
            let phase = std::f32::consts::TAU * HZ * (i as f32 + 0.3) / RATE;
            (gain * phase.sin()).clamp(-1.0, 1.0)
        })
    }

    /// The scope pane's rows after `samples` pass through a mono tee.
    fn wave(samples: impl Iterator<Item = f32>) -> Vec<String> {
        let (mut tx, mut tee) = audio::output_tee(FFT_FRAMES, 1);
        let mut scopes = Scopes::new(&tee, RATE, 0.0);
        for sample in samples {
            tx.push(sample).unwrap();
        }
        scopes.update(&mut tee, Some(HZ));
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 32));
        scopes.draw(&mut buf, Rect::new(0, 0, 24, 16), Rect::new(0, 16, 24, 16));
        (0..16)
            .map(|y| (0..24).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn colours_survive_the_version_bridge() {
        let pairs = [
            (OldColor::Reset, Color::Reset),
            (OldColor::DarkGray, Color::DarkGray),
            (OldColor::Indexed(42), Color::Indexed(42)),
            (OldColor::Rgb(1, 2, 3), Color::Rgb(1, 2, 3)),
        ];
        for (old, new) in pairs {
            assert_eq!(color(old), new);
        }
    }

    #[test]
    fn the_scope_holds_still_across_read_phases() {
        assert_eq!(wave(sine(1.0, 0, 1000)), wave(sine(1.0, 37, 1000)));
    }

    #[test]
    fn clipping_draws_a_flat_top() {
        let top = |rows: &[String]| rows[0].chars().filter(|c| !matches!(c, ' ' | '⠀')).count();
        let clean = top(&wave(sine(1.0, 0, 1000)));
        let clipped = top(&wave(sine(4.0, 0, 1000)));
        assert!(clipped > 2 * clean, "clean {clean}, clipped {clipped}");
    }
}
