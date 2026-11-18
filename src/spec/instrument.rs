/// timbre and envelope
pub struct Instrument {
    /// full waveform cycle, -1.0..1.0
    pub wave: Vec<f32>,
    /// 0.0..1.0
    pub(crate) attack: Vec<f32>,
    /// 1.0..sustain_level
    pub(crate) decay: Vec<f32>,
    /// 0.0..1.0
    pub(crate) sustain_level: f32,
    /// sustain_level..0.0
    pub(crate) release: Vec<f32>,
    /// notes sounding at once; 1 is mono
    pub(crate) voice_count: usize,
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
