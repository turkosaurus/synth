use std::sync::Arc;
use std::time::Duration;

use super::instrument::{Event, Freqs, Instrument, Timeline};

enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

// one sounding note
struct Note {
    instrument: Arc<Instrument>,
    /// 0.0-1.0, scaling the envelope peak
    velocity: f32,
    phase: f32, // 0.0..1.0
    step: f32,  // phase increment per sample
    stage: Stage,
    idx: usize,                  // index into the current envelope segment
    held: usize,                 // how many samples have been held in sustain
    hold_samples: Option<usize>, // how many samples to hold in sustain
    level: f32,                  // current envelope level
    release_scale: f32,          // scale factor for release envelope
}

impl Note {
    // `hold: None` means the note sustains until something calls off(),
    // which is how the sequencer drives it
    fn new(
        instrument: Arc<Instrument>,
        freq: f32,
        velocity: f32,
        sample_rate: f32,
        hold: Option<Duration>,
    ) -> Self {
        Self {
            velocity,
            step: freq / sample_rate,
            hold_samples: hold.map(|d| (d.as_secs_f32() * sample_rate) as usize),
            instrument,
            phase: 0.0,
            stage: Stage::Attack,
            idx: 0,
            held: 0,
            level: 0.0,
            release_scale: 1.0,
        }
    }

    // release starts from wherever the envelope actually was, so a key
    // lifted mid-attack does not jump up to sustain level first
    fn off(&mut self) {
        self.release_scale = if self.instrument.sustain_level > 0.0 {
            self.level / self.instrument.sustain_level
        } else {
            1.0
        };
        self.stage = Stage::Release;
        self.idx = 0;
    }

    // the loop falls through zero-length segments, e.g. attackMS = 0
    fn envelope(&mut self) -> f32 {
        loop {
            match self.stage {
                Stage::Attack => {
                    if let Some(&value) = self.instrument.attack.get(self.idx) {
                        self.idx += 1;
                        self.level = value;
                        return value;
                    }
                    self.stage = Stage::Decay;
                    self.idx = 0;
                }
                Stage::Decay => {
                    if let Some(&value) = self.instrument.decay.get(self.idx) {
                        self.idx += 1;
                        self.level = value;
                        return value;
                    }
                    self.stage = Stage::Sustain;
                    self.idx = 0;
                }
                Stage::Sustain => {
                    if self.hold_samples.is_some_and(|limit| self.held >= limit) {
                        self.off();
                        continue;
                    }
                    self.held += 1;
                    self.level = self.instrument.sustain_level;
                    return self.level;
                }
                Stage::Release => {
                    if let Some(&value) = self.instrument.release.get(self.idx) {
                        self.idx += 1;
                        self.level = value * self.release_scale;
                        return self.level;
                    }
                    self.stage = Stage::Done;
                }
                Stage::Done => {
                    self.level = 0.0;
                    return 0.0;
                }
            }
        }
    }

    fn wave(&self) -> f32 {
        let n = self.instrument.wave.len();
        let pos = self.phase * n as f32;
        let i = pos as usize % n;
        let j = (i + 1) % n;
        self.instrument.wave[i]
            + (self.instrument.wave[j] - self.instrument.wave[i]) * (pos - pos.floor())
    }

    fn next(&mut self) -> f32 {
        let env = self.envelope();
        let sample = self.wave() * env * self.velocity;
        self.phase = (self.phase + self.step).fract();
        sample
    }
}

/// Where a player's events come from. Downstream handling is identical, so
/// the sequencer and a live input differ only here.
enum Feed {
    Timeline {
        events: Vec<(u64, Event)>,
        next: usize,
    },
    Live(rtrb::Consumer<Event>),
}

pub struct Player {
    instruments: Vec<Arc<Instrument>>,
    freqs: Freqs,
    feed: Feed,
    sample_rate: f32,
    note: Option<Note>,
    /// which note is sounding, so NoteOff can be matched and a view can
    /// report it
    sounding: Option<u8>,
    clock: u64,
    total_samples: u64,
}

impl Player {
    pub fn new(timeline: Timeline, freqs: Freqs, sample_rate: f32) -> Self {
        Self {
            instruments: timeline.instruments,
            freqs,
            feed: Feed::Timeline {
                events: timeline.events,
                next: 0,
            },
            sample_rate,
            note: None,
            sounding: None,
            clock: 0,
            total_samples: timeline.total_samples,
        }
    }

    pub fn live(
        instruments: Vec<Arc<Instrument>>,
        freqs: Freqs,
        events: rtrb::Consumer<Event>,
        sample_rate: f32,
    ) -> Self {
        Self {
            instruments,
            freqs,
            feed: Feed::Live(events),
            sample_rate,
            note: None,
            sounding: None,
            clock: 0,
            total_samples: u64::MAX,
        }
    }

    pub fn sounding(&self) -> Option<u8> {
        self.sounding
    }

    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    // returns an owned event so the borrow of `feed` ends before `apply`
    // needs &mut self
    fn pull(&mut self) -> Option<Event> {
        let clock = self.clock;
        match &mut self.feed {
            Feed::Timeline { events, next } => {
                let (at, event) = events.get(*next)?;
                if *at <= clock {
                    *next += 1;
                    Some(event.clone())
                } else {
                    None
                }
            }
            Feed::Live(consumer) => consumer.pop().ok(),
        }
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::NoteOn {
                instrument,
                note,
                velocity,
            } => {
                // a MIDI note-on with velocity 0 means note-off
                if velocity == 0 {
                    self.release(note);
                    return;
                }
                let Some(instrument) = self.instruments.get(instrument) else {
                    return;
                };
                let Some(&freq) = self.freqs.get(note as usize) else {
                    return;
                };
                // mono: a new note replaces whatever was sounding
                self.sounding = Some(note);
                self.note = Some(Note::new(
                    Arc::clone(instrument),
                    freq,
                    velocity as f32 / 127.0,
                    self.sample_rate,
                    None,
                ));
            }
            Event::NoteOff { note, .. } => self.release(note),
        }
    }

    // Press a, press s, release a. Mono already replaced a's note with s's,
    // so that release names something no longer playing and acting on it
    // would cut the note that is.
    fn release(&mut self, note: u8) {
        if self.sounding != Some(note) {
            return;
        }
        self.sounding = None;
        if let Some(note) = &mut self.note {
            note.off();
        }
    }

    fn next(&mut self) -> f32 {
        // several events can land on the same sample, so drain rather than peek
        while let Some(event) = self.pull() {
            self.apply(event);
        }

        self.clock += 1;
        self.note.as_mut().map_or(0.0, Note::next)
    }
}

/// What one callback produced. Accumulated in locals so the atomics are
/// touched once per callback instead of once per sample.
#[derive(Debug, Default, PartialEq)]
pub struct Tally {
    pub frames: u32,
    pub clips: u64,
    pub peak: f32,
}

/// Lay one sample per frame across every channel. Split out of the callback
/// so the interleaving can be tested without an audio device.
pub(super) fn fill(out: &mut [f32], channels: usize, player: &mut Player) -> Tally {
    let mut tally = Tally::default();

    for frame in out.chunks_mut(channels) {
        let sample = player.next();
        if sample.abs() > 1.0 {
            tally.clips += 1;
        }
        tally.peak = tally.peak.max(sample.abs());
        tally.frames += 1;
        for channel in frame.iter_mut() {
            *channel = sample;
        }
    }
    tally
}

#[cfg(test)]
mod tests {
    use super::*;

    // 4-entry wave so interpolation is checkable by hand
    fn instrument() -> Instrument {
        Instrument {
            wave: vec![0.0, 1.0, 0.0, -1.0],
            attack: vec![0.0, 0.5],
            decay: vec![1.0, 0.85],
            sustain_level: 0.7,
            release: vec![0.7, 0.35],
        }
    }

    // 4 Hz sample rate makes hold = 1s exactly 4 samples
    fn note(instrument: Instrument) -> Note {
        Note::new(
            Arc::new(instrument),
            1.0,
            1.0,
            4.0,
            Some(Duration::from_secs(1)),
        )
    }

    #[test]
    fn envelope_walks_every_stage_in_order() {
        let mut note = note(instrument());
        let got: Vec<f32> = (0..12).map(|_| note.envelope()).collect();
        assert_eq!(
            got,
            vec![
                0.0, 0.5, // attack
                1.0, 0.85, // decay
                0.7, 0.7, 0.7, 0.7, // sustain, held 4 samples
                0.7, 0.35, // release
                0.0, 0.0, // done
            ]
        );
    }

    #[test]
    fn zero_length_segment_falls_through() {
        let mut instrument = instrument();
        instrument.attack = vec![];
        // with no attack samples the first value must come from decay
        assert_eq!(note(instrument).envelope(), 1.0);
    }

    #[test]
    fn release_resumes_from_the_current_level() {
        let mut note = note(instrument());
        assert_eq!(note.envelope(), 0.0);
        assert_eq!(note.envelope(), 0.5);

        // lifted mid-attack: release must start at 0.5, not jump to sustain
        note.off();
        assert!((note.envelope() - 0.5).abs() < 1e-6);
    }

    // a voice stolen mid-release hands its level to the next note, so the
    // level has to follow the release down rather than stay at sustain
    #[test]
    fn release_tracks_the_falling_level() {
        let mut note = note(instrument());
        for _ in 0..5 {
            note.envelope(); // attack, decay, into sustain
        }
        note.off();

        assert_eq!(note.envelope(), 0.7);
        assert_eq!(note.envelope(), 0.35);
        assert_eq!(note.level, 0.35);

        note.envelope(); // release exhausted
        assert_eq!(note.level, 0.0);
    }

    #[test]
    fn wave_interpolates_and_wraps() {
        let mut note = note(instrument());

        note.phase = 0.0;
        assert_eq!(note.wave(), 0.0);

        note.phase = 0.125; // halfway between wave[0] and wave[1]
        assert_eq!(note.wave(), 0.5);

        note.phase = 0.25;
        assert_eq!(note.wave(), 1.0);

        note.phase = 0.875; // halfway from wave[3] back around to wave[0]
        assert_eq!(note.wave(), -0.5);
    }

    // an instrument whose envelope holds 1.0 forever, so fill() output is the
    // waveform alone and easy to predict
    fn flat_instrument() -> Instrument {
        Instrument {
            wave: vec![0.25, 0.5, 0.75, 1.0],
            attack: vec![],
            decay: vec![],
            sustain_level: 1.0,
            release: vec![],
        }
    }

    // a Player whose timeline is a single note-on at sample 0
    // freqs[1] = 1.0 so step is 1/4 of the table per sample at rate 4.0
    fn freqs() -> Freqs {
        let mut freqs = vec![0.0; 128];
        freqs[1] = 1.0;
        freqs
    }

    fn player(instrument: Instrument) -> Player {
        Player::new(
            Timeline {
                instruments: vec![Arc::new(instrument)],
                events: vec![(
                    0,
                    Event::NoteOn {
                        instrument: 0,
                        note: 1,
                        velocity: 127,
                    },
                )],
                total_samples: 64,
            },
            freqs(),
            4.0,
        )
    }

    #[test]
    fn a_live_feed_drains_the_queue() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut player = Player::live(vec![Arc::new(flat_instrument())], freqs(), rx, 4.0);

        // nothing queued yet, so silence
        assert_eq!(player.next(), 0.0);

        tx.push(Event::NoteOn {
            instrument: 0,
            note: 1,
            velocity: 127,
        })
        .unwrap();
        assert_eq!(player.next(), 0.25); // wave[0], envelope held at 1.0

        tx.push(Event::NoteOff {
            instrument: 0,
            note: 1,
        })
        .unwrap();
        // release is empty in flat_instrument, so the note goes straight to Done
        assert_eq!(player.next(), 0.0);
    }

    #[test]
    fn a_noteoff_for_a_note_that_is_not_sounding_is_ignored() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut player = Player::live(vec![Arc::new(flat_instrument())], freqs(), rx, 4.0);

        tx.push(Event::NoteOn {
            instrument: 0,
            note: 1,
            velocity: 127,
        })
        .unwrap();
        assert_eq!(player.next(), 0.25);

        // a stale release from a key that was already stolen must not cut the
        // note that is actually sounding
        tx.push(Event::NoteOff {
            instrument: 0,
            note: 99,
        })
        .unwrap();
        assert_eq!(player.next(), 0.5); // still playing, advanced one entry
        assert_eq!(player.sounding(), Some(1));
    }

    #[test]
    fn velocity_zero_means_note_off() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut player = Player::live(vec![Arc::new(flat_instrument())], freqs(), rx, 4.0);

        tx.push(Event::NoteOn {
            instrument: 0,
            note: 1,
            velocity: 127,
        })
        .unwrap();
        assert_eq!(player.next(), 0.25);

        // the MIDI convention: a note-on at velocity 0 releases
        tx.push(Event::NoteOn {
            instrument: 0,
            note: 1,
            velocity: 0,
        })
        .unwrap();
        assert_eq!(player.next(), 0.0);
        assert_eq!(player.sounding(), None);
    }

    #[test]
    fn velocity_scales_the_output() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut player = Player::live(vec![Arc::new(flat_instrument())], freqs(), rx, 4.0);

        tx.push(Event::NoteOn {
            instrument: 0,
            note: 1,
            velocity: 64,
        })
        .unwrap();
        // wave[0] is 0.25, envelope 1.0, velocity 64/127
        let expected = 0.25 * (64.0 / 127.0);
        assert!((player.next() - expected).abs() < 1e-6);
    }

    #[test]
    fn a_live_note_replaces_the_one_sounding() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut player = Player::live(vec![Arc::new(flat_instrument())], freqs(), rx, 4.0);

        let on = |note| Event::NoteOn {
            instrument: 0,
            note,
            velocity: 127,
        };

        tx.push(on(1)).unwrap();
        player.next();
        player.next(); // advance into the table

        // a second note-on resets phase, so we are back at wave[0]
        tx.push(on(1)).unwrap();
        assert_eq!(player.next(), 0.25);
    }

    #[test]
    fn fill_writes_one_sample_across_every_channel() {
        let mut out = [0.0f32; 8];
        let tally = fill(&mut out, 2, &mut player(flat_instrument()));

        assert_eq!(tally.frames, 4);
        // each pair is one frame: both channels carry the same sample
        assert_eq!(out, [0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0]);
    }

    #[test]
    fn fill_handles_mono_and_four_channels() {
        let mut mono = [0.0f32; 4];
        assert_eq!(fill(&mut mono, 1, &mut player(flat_instrument())).frames, 4);
        assert_eq!(mono, [0.25, 0.5, 0.75, 1.0]);

        let mut quad = [0.0f32; 8];
        assert_eq!(fill(&mut quad, 4, &mut player(flat_instrument())).frames, 2);
        assert_eq!(quad, [0.25, 0.25, 0.25, 0.25, 0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn fill_tracks_peak_and_clipping() {
        let mut instrument = flat_instrument();
        instrument.wave = vec![0.5, -2.0, 1.5, 0.1];

        let mut out = [0.0f32; 8];
        let tally = fill(&mut out, 2, &mut player(instrument));

        assert_eq!(tally.frames, 4);
        assert_eq!(tally.clips, 2); // -2.0 and 1.5 both exceed full scale
        assert_eq!(tally.peak, 2.0); // peak is absolute, so -2.0 wins
    }

    #[test]
    fn fill_leaves_a_short_trailing_frame_partially_written() {
        // cpal should always hand over whole frames; this pins the behavior
        // if it ever does not
        let mut out = [0.0f32; 5];
        let tally = fill(&mut out, 2, &mut player(flat_instrument()));

        assert_eq!(tally.frames, 3); // two full frames plus a short one
        assert_eq!(out, [0.25, 0.25, 0.5, 0.5, 0.75]);
    }

    #[test]
    fn phase_advances_by_frequency_over_sample_rate() {
        let mut note = Note::new(Arc::new(instrument()), 440.0, 1.0, 48000.0, None);
        assert!((note.step - 440.0 / 48000.0).abs() < 1e-9);

        let before = note.phase;
        note.next();
        assert!((note.phase - before - note.step).abs() < 1e-6);
    }
}
