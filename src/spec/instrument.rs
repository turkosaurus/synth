/// timbre and envelope
pub struct Instrument {
    pub wave: Wavetable,
    /// 0.0 to 1.0
    pub(crate) attack: Segment,
    /// 1.0 to sustain_level
    pub(crate) decay: Segment,
    /// 0.0..1.0
    pub(crate) sustain_level: f32,
    /// sustain_level to 0.0
    pub(crate) release: Segment,
    /// notes sounding at once; 1 is mono
    pub(crate) voice_count: usize,
}

/// Waveform cycles of equal length, one after another in one Vec, so a read
/// follows no pointer. Each frame is one cycle, -1.0..1.0, starting where
/// it first rises through 0 when it does. A single waveform is a table of
/// one frame, and playback reads frame 0.
pub struct Wavetable {
    pub(crate) frames: Vec<f32>,
    pub(crate) points: usize,
    /// one curve per transition, 0..1 at evenly spaced points from t = 0 to
    /// t = 1; reused in order when there are fewer slopes than transitions,
    /// and linear when there are none
    pub(crate) slopes: Vec<Vec<f32>>,
    /// 0..1, from the first frame to the last
    pub(crate) position: f32,
    /// moves `position` either side of its value while notes sound
    pub(crate) position_mod: Option<Lfo>,
}

/// A baked LFO: one cycle read at `rate` Hz, moving a parameter `depth`
/// either side of its value, in that parameter's units.
#[derive(Clone)]
pub struct Lfo {
    pub(crate) cycle: Vec<f32>,
    pub(crate) rate: f32,
    pub(crate) depth: f32,
}

impl Wavetable {
    pub fn frame_count(&self) -> usize {
        self.frames.len().checked_div(self.points).unwrap_or(0)
    }

    /// One cycle, `points` long.
    pub fn frame(&self, k: usize) -> &[f32] {
        &self.frames[k * self.points..(k + 1) * self.points]
    }

    pub(crate) fn frame_mut(&mut self, k: usize) -> &mut [f32] {
        &mut self.frames[k * self.points..(k + 1) * self.points]
    }

    /// The two frames either side of the table's own `position`, and how
    /// far toward the second.
    pub(crate) fn blend(&self) -> (usize, usize, f32) {
        self.blend_at(self.position)
    }

    /// The two frames either side of `position`, and how far toward the
    /// second, through that transition's slope.
    pub(crate) fn blend_at(&self, position: f32) -> (usize, usize, f32) {
        let count = self.frame_count();
        if count < 2 {
            return (0, 0, 0.0);
        }
        let x = position.clamp(0.0, 1.0) * (count - 1) as f32;
        let k = (x as usize).min(count - 2);
        let along = x - k as f32;
        let mix = match self.slopes.len() {
            0 => along,
            n => read(&self.slopes[k % n], along),
        };
        (k, k + 1, mix)
    }
}

/// `curve` at `t` in 0..1, read between the two points either side.
fn read(curve: &[f32], t: f32) -> f32 {
    let last = curve.len().saturating_sub(1);
    if last == 0 {
        return curve.first().copied().unwrap_or(t);
    }
    let pos = t.clamp(0.0, 1.0) * last as f32;
    let i = (pos as usize).min(last - 1);
    curve[i] + (curve[i + 1] - curve[i]) * (pos - i as f32)
}

/// One cycle as a table of one frame.
impl From<Vec<f32>> for Wavetable {
    fn from(cycle: Vec<f32>) -> Self {
        let points = cycle.len();
        Self {
            frames: cycle,
            points,
            slopes: Vec::new(),
            position: 0.0,
            position_mod: None,
        }
    }
}

/// One envelope stage: its curve, baked once at a fixed number of points
/// whatever the stage's length, and the length itself. Playback stretches
/// the curve over as many samples as `ms` is at the device's rate, so
/// neither the length nor the rate is baked in.
pub struct Segment {
    /// how far from the stage's start level toward its end level, 0.0..1.0,
    /// at evenly spaced points from t = 0 to t = 1 inclusive
    pub(crate) curve: Vec<f32>,
    pub(crate) ms: f64,
}

impl Segment {
    /// The stage's length in samples at `sample_rate`.
    pub(crate) fn samples(&self, sample_rate: f64) -> usize {
        (sample_rate * self.ms / 1000.0) as usize
    }

    /// How far along the stage is at sample `i` of `len`, read between the
    /// two baked points either side. Arithmetic only, for the audio thread.
    pub(crate) fn along(&self, i: usize, len: usize) -> f32 {
        let last = self.curve.len().saturating_sub(1);
        if last == 0 || len == 0 {
            return self.curve.first().copied().unwrap_or(0.0);
        }
        // f64, since i times the point count passes f32's exact integers
        // within a few seconds of stage
        let pos = i as f64 * last as f64 / len as f64;
        let k = pos as usize;
        if k >= last {
            return self.curve[last];
        }
        let frac = (pos - k as f64) as f32;
        self.curve[k] + (self.curve[k + 1] - self.curve[k]) * frac
    }
}

/// MIDI-shaped on purpose: note numbers rather than frequencies, and an
/// instrument on every event the way MIDI puts a channel on every message.
/// That keeps a MIDI input close to a passthrough, puts tuning in one place
/// (the audio side), and lets a release find the voice pool it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    NoteOn {
        instrument: usize,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        instrument: usize,
        note: u8,
    },
    /// MIDI All Notes Off (CC 123): every voice on the instrument releases,
    /// and the release stage still plays.
    AllNotesOff {
        instrument: usize,
    },
    /// MIDI All Sound Off (CC 120): every voice on the instrument stops on
    /// the next sample, with no release stage.
    AllSoundOff {
        instrument: usize,
    },
}

/// Events stamped with the sample they fire on. A single sustained note is
/// just a two-event timeline, so playback has one code path.
pub struct Timeline {
    pub instruments: Vec<Instrument>,
    pub events: Vec<(u64, Event)>,
    pub(crate) total_samples: u64,
}

/// Hz per MIDI note, baked from Lua so temperament lives there and every
/// input resolves pitch the same way.
pub type Freqs = Vec<f32>;
