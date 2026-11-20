/// timbre and envelope
pub struct Instrument {
    /// full waveform cycle, -1.0..1.0
    pub wave: Vec<f32>,
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
