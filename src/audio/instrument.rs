use std::sync::Arc;

// timbre and envelope
pub struct Instrument {
    pub wave: Vec<f32>,     // full waveform cycle, -1.0..1.0
    pub attack: Vec<f32>,   // 0.0..1.0
    pub decay: Vec<f32>,    // 1.0..sustain_level
    pub sustain_level: f32, // 0.0..1.0
    pub release: Vec<f32>,  // sustain_level..0.0
}

/// MIDI-shaped on purpose: note numbers rather than frequencies, and a note
/// on NoteOff. That keeps a MIDI input close to a passthrough, puts tuning in
/// one place (the audio side), and means polyphony is not a format change.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    NoteOn { instrument: usize, note: u8, velocity: u8 },
    NoteOff { note: u8 },
}

/// Events stamped with the sample they fire on. A single sustained note is
/// just a two-event timeline, so playback has one code path.
pub struct Timeline {
    pub instruments: Vec<Arc<Instrument>>,
    pub events: Vec<(u64, Event)>,
    pub total_samples: u64,
}

/// Hz per MIDI note, baked from Lua so temperament lives there and every
/// input resolves pitch the same way.
pub type Freqs = Vec<f32>;
