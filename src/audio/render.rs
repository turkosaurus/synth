use std::sync::Arc;
use std::time::Duration;

use super::instrument::{Event, Freqs, Instrument, Timeline};

/// How long a stolen voice's velocity takes to ease to the new note's own.
/// Short enough to hear as the new note, long enough not to click.
const GLIDE: Duration = Duration::from_millis(5);

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
    /// 0.0-1.0, scaling the envelope peak; differs from `target` only while
    /// easing after a steal
    velocity: f32,
    target: f32,
    glide: f32, // velocity moved per sample toward `target`
    ramp: f32,  // samples in GLIDE at this sample rate
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
            target: velocity,
            glide: 0.0,
            ramp: (GLIDE.as_secs_f32() * sample_rate).max(1.0),
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
        let gap = self.target - self.velocity;
        self.velocity += gap.clamp(-self.glide, self.glide);
        sample
    }

    // A stolen voice hands over where it was, so the new note picks up the
    // waveform and envelope mid-flight rather than jumping to zero: no
    // click. Both notes share a pool, so they read the same wavetable.
    //
    // The level carried is what was audible, env × velocity, re-expressed at
    // this note's velocity. Attack resumes at its first entry at or above
    // that level, which assumes a rising attack; a curve that dips takes its
    // first crossing.
    //
    // The envelope cannot always land on that level: a soft note stealing a
    // loud one needs more than its peak, and an instrument with no attack
    // starts at decay. So velocity starts wherever makes the first sample
    // match, then eases to this note's own over GLIDE.
    fn take_over(&mut self, from: &Note) {
        let audible = from.level * from.velocity;
        let level = audible / self.target;
        self.phase = from.phase;
        self.level = level;
        let attack = &self.instrument.attack;
        self.idx = attack
            .iter()
            .position(|&value| value >= level)
            .unwrap_or(attack.len());

        let first = attack
            .get(self.idx)
            .or(self.instrument.decay.first())
            .copied()
            .unwrap_or(self.instrument.sustain_level);
        if first > 0.0 {
            self.velocity = audible / first;
            self.glide = (self.target - self.velocity).abs() / self.ramp;
        }
    }

    fn releasing(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Done)
    }

    fn done(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }
}

/// One slot in a pool. Pitch and age sit beside the note so the allocator
/// can find a retrigger or the oldest voice without asking the note.
struct Voice {
    note: Note,
    pitch: u8,
    age: u64, // clock at note-on
}

/// One instrument's voices. Sized once at load, so a note-on on the audio
/// thread fills a slot and never allocates.
struct Pool {
    instrument: Arc<Instrument>,
    voices: Vec<Option<Voice>>,
    /// Voices at different pitches rarely peak together, so N of them sum to
    /// about sqrt(N) times one. Fixed at load: a gain that followed the live
    /// note count would step every other note's level on each note-on.
    gain: f32,
}

impl Pool {
    fn new(instrument: Arc<Instrument>) -> Self {
        let voices = (0..instrument.voice_count).map(|_| None).collect();
        let gain = 1.0 / (instrument.voice_count.max(1) as f32).sqrt();
        Self {
            instrument,
            voices,
            gain,
        }
    }

    // A retrigger takes its own pitch's voice, then a free slot. A full pool
    // steals the oldest voice, releasing ones first since they are already
    // on their way out. At voice_count = 1 that is mono.
    fn slot(&self, pitch: u8) -> Option<usize> {
        let retrigger = self
            .voices
            .iter()
            .position(|v| v.as_ref().is_some_and(|v| v.pitch == pitch));
        let free = || self.voices.iter().position(Option::is_none);
        let steal = || {
            self.voices
                .iter()
                .enumerate()
                .filter_map(|(i, v)| v.as_ref().map(|v| (i, v)))
                .min_by_key(|(_, v)| (!v.note.releasing(), v.age))
                .map(|(i, _)| i)
        };
        retrigger.or_else(free).or_else(steal)
    }

    // Matched by pitch, so a release for a stolen note finds nothing. A
    // voice already releasing is left alone so its fade does not restart.
    fn release(&mut self, pitch: u8) {
        let held = self
            .voices
            .iter_mut()
            .flatten()
            .find(|v| v.pitch == pitch && !v.note.releasing());
        if let Some(voice) = held {
            voice.note.off();
        }
    }

    fn release_all(&mut self) {
        for voice in self.voices.iter_mut().flatten() {
            if !voice.note.releasing() {
                voice.note.off();
            }
        }
    }

    fn next(&mut self) -> f32 {
        let mut sum = 0.0;
        for slot in &mut self.voices {
            if let Some(voice) = slot {
                sum += voice.note.next();
                if voice.note.done() {
                    *slot = None;
                }
            }
        }
        sum * self.gain
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
    /// one per instrument, indexed the same way events name instruments
    pools: Vec<Pool>,
    freqs: Freqs,
    feed: Feed,
    sample_rate: f32,
    clock: u64,
    total_samples: u64,
}

impl Player {
    pub fn new(timeline: Timeline, freqs: Freqs, sample_rate: f32) -> Self {
        Self {
            pools: timeline.instruments.into_iter().map(Pool::new).collect(),
            freqs,
            feed: Feed::Timeline {
                events: timeline.events,
                next: 0,
            },
            sample_rate,
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
            pools: instruments.into_iter().map(Pool::new).collect(),
            freqs,
            feed: Feed::Live(events),
            sample_rate,
            clock: 0,
            total_samples: u64::MAX,
        }
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
                    self.release(instrument, note);
                    return;
                }
                let Some(pool) = self.pools.get_mut(instrument) else {
                    return;
                };
                let Some(&freq) = self.freqs.get(note as usize) else {
                    return;
                };
                let Some(slot) = pool.slot(note) else {
                    return; // a pool with no voices
                };
                let mut started = Note::new(
                    Arc::clone(&pool.instrument),
                    freq,
                    velocity as f32 / 127.0,
                    self.sample_rate,
                    None,
                );
                if let Some(stolen) = &pool.voices[slot] {
                    started.take_over(&stolen.note);
                }
                pool.voices[slot] = Some(Voice {
                    note: started,
                    pitch: note,
                    age: self.clock,
                });
            }
            Event::NoteOff { instrument, note } => self.release(instrument, note),
            Event::AllNotesOff { instrument } => {
                if let Some(pool) = self.pools.get_mut(instrument) {
                    pool.release_all();
                }
            }
        }
    }

    fn release(&mut self, instrument: usize, note: u8) {
        if let Some(pool) = self.pools.get_mut(instrument) {
            pool.release(note);
        }
    }

    fn next(&mut self) -> f32 {
        // several events can land on the same sample, so drain rather than peek
        while let Some(event) = self.pull() {
            self.apply(event);
        }

        self.clock += 1;
        self.pools.iter_mut().map(Pool::next).sum()
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
            voice_count: 1,
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
            voice_count: 1,
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
    fn a_retrigger_continues_the_waveform() {
        let (mut tx, mut player) = pooled(1);
        tx.push(on(1)).unwrap();
        player.next();
        player.next(); // advance into the table

        // the retrigger keeps the phase, so this is wave[2], not wave[0]
        tx.push(on(1)).unwrap();
        assert_eq!(player.next(), 0.75);
    }

    fn rising() -> Instrument {
        let mut instrument = instrument();
        instrument.attack = vec![0.0, 0.25, 0.5, 0.75];
        instrument
    }

    #[test]
    fn take_over_resumes_attack_at_the_handed_over_level() {
        let mut from = Note::new(Arc::new(rising()), 1.0, 1.0, 4.0, None);
        from.level = 0.5;
        from.phase = 0.3;

        let mut note = Note::new(Arc::new(rising()), 2.0, 1.0, 4.0, None);
        note.take_over(&from);
        assert_eq!(note.phase, 0.3);
        assert_eq!(note.envelope(), 0.5); // attack[2], not attack[0]
    }

    #[test]
    fn take_over_matches_audible_level_across_velocities() {
        let mut from = Note::new(Arc::new(rising()), 1.0, 0.5, 4.0, None);
        from.level = 0.5; // audible at 0.5 × 0.5 = 0.25

        let mut note = Note::new(Arc::new(rising()), 1.0, 1.0, 4.0, None);
        note.take_over(&from);
        assert_eq!(note.envelope(), 0.25); // 0.25 × 1.0 matches
    }

    #[test]
    fn a_soft_note_stealing_a_loud_one_eases_down() {
        let mut from = Note::new(Arc::new(rising()), 1.0, 1.0, 4.0, None);
        from.level = 0.7; // sustaining at full velocity
        from.phase = 0.25; // wave[1] = 1.0, so samples read as env × velocity

        let soft = 40.0 / 127.0;
        let mut note = Note::new(Arc::new(rising()), 0.0, soft, 4.0, None);
        note.take_over(&from);

        // 0.7 is above anything the soft note's envelope can reach, so
        // velocity carries it; the first sample matches, with no drop
        assert!((note.next() - 0.7).abs() < 1e-6);
        // GLIDE is under one sample at this rate, so it lands at once
        assert_eq!(note.velocity, soft);
    }

    #[test]
    fn take_over_with_no_attack_eases_up_from_the_handed_over_level() {
        let mut instrument = rising();
        instrument.attack = vec![];
        let mut from = Note::new(Arc::new(rising()), 1.0, 1.0, 4.0, None);
        from.level = 0.2;
        from.phase = 0.25;

        let mut note = Note::new(Arc::new(instrument), 0.0, 1.0, 4.0, None);
        note.take_over(&from);
        // decay starts at 1.0, so velocity starts at 0.2 instead of jumping
        assert!((note.next() - 0.2).abs() < 1e-6);
        assert_eq!(note.velocity, 1.0);
    }

    // A live player on flat_instrument with `voice_count` voices. Only
    // freqs[1] is nonzero, so any other note holds still at wave[0] = 0.25.
    fn pooled(voice_count: usize) -> (rtrb::Producer<Event>, Player) {
        let (tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut instrument = flat_instrument();
        instrument.voice_count = voice_count;
        (
            tx,
            Player::live(vec![Arc::new(instrument)], freqs(), rx, 4.0),
        )
    }

    fn on(note: u8) -> Event {
        Event::NoteOn {
            instrument: 0,
            note,
            velocity: 127,
        }
    }

    fn off(note: u8) -> Event {
        Event::NoteOff {
            instrument: 0,
            note,
        }
    }

    fn pitches(player: &Player) -> Vec<Option<u8>> {
        let voices = &player.pools[0].voices;
        voices.iter().map(|v| v.as_ref().map(|v| v.pitch)).collect()
    }

    #[test]
    fn a_pool_is_scaled_by_one_over_root_voice_count() {
        let (mut tx, mut player) = pooled(4);
        tx.push(on(2)).unwrap();
        assert_eq!(player.next(), 0.25 * 0.5);
    }

    #[test]
    fn two_pitches_sound_together() {
        let (mut tx, mut player) = pooled(2);
        tx.push(on(1)).unwrap();
        player.next();

        tx.push(on(2)).unwrap();
        // note 1 has moved on to wave[1]; note 2 adds its wave[0]
        let gain = 1.0 / 2.0f32.sqrt();
        assert!((player.next() - (0.5 + 0.25) * gain).abs() < 1e-6);
        assert_eq!(pitches(&player), vec![Some(1), Some(2)]);
    }

    #[test]
    fn a_retrigger_reuses_its_voice_with_room_to_spare() {
        let (mut tx, mut player) = pooled(2);
        tx.push(on(1)).unwrap();
        player.next();
        tx.push(on(1)).unwrap();
        player.next();
        assert_eq!(pitches(&player), vec![Some(1), None]);
    }

    #[test]
    fn a_second_release_does_not_restart_the_fade() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut instrument = flat_instrument();
        instrument.release = vec![1.0, 0.5, 0.25];
        let mut player = Player::live(vec![Arc::new(instrument)], freqs(), rx, 4.0);

        tx.push(on(2)).unwrap();
        player.next();
        tx.push(off(2)).unwrap();
        assert_eq!(player.next(), 0.25); // wave[0] × release[0]
        tx.push(off(2)).unwrap();
        assert_eq!(player.next(), 0.25 * 0.5); // release[1], not release[0]
    }

    #[test]
    fn a_retrigger_while_releasing_resumes_from_the_fade() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut instrument = flat_instrument();
        instrument.release = vec![1.0, 0.5, 0.25];
        let mut player = Player::live(vec![Arc::new(instrument)], freqs(), rx, 4.0);

        tx.push(on(2)).unwrap();
        player.next();
        tx.push(off(2)).unwrap();
        player.next(); // release[0]
        assert_eq!(player.next(), 0.25 * 0.5); // release[1]

        // the retrigger starts at the faded level, not full, then eases up
        tx.push(on(2)).unwrap();
        assert_eq!(player.next(), 0.25 * 0.5);
        assert_eq!(player.next(), 0.25);
        assert!(!player.pools[0].voices[0].as_ref().unwrap().note.releasing());
    }

    #[test]
    fn a_full_pool_steals_the_oldest_voice() {
        let (mut tx, mut player) = pooled(2);
        for note in [1, 2, 3] {
            tx.push(on(note)).unwrap();
            player.next();
        }
        assert_eq!(pitches(&player), vec![Some(3), Some(2)]);

        // note 1 was stolen, so its release finds nothing to cut
        tx.push(off(1)).unwrap();
        player.next();
        assert_eq!(pitches(&player), vec![Some(3), Some(2)]);
    }

    #[test]
    fn a_full_pool_steals_a_releasing_voice_before_an_older_held_one() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut instrument = flat_instrument();
        instrument.voice_count = 2;
        instrument.release = vec![1.0; 16]; // long enough to still be fading
        let mut player = Player::live(vec![Arc::new(instrument)], freqs(), rx, 4.0);

        for event in [on(1), on(2), off(2), on(3)] {
            tx.push(event).unwrap();
            player.next();
        }
        assert_eq!(pitches(&player), vec![Some(1), Some(3)]);
    }

    #[test]
    fn a_finished_voice_frees_its_slot() {
        let (mut tx, mut player) = pooled(2);
        tx.push(on(1)).unwrap();
        player.next();

        // flat_instrument has no release, so the note is done at once
        tx.push(off(1)).unwrap();
        player.next();
        assert_eq!(pitches(&player), vec![None, None]);
    }

    #[test]
    fn mono_steals_on_every_new_pitch() {
        let (mut tx, mut player) = pooled(1);
        tx.push(on(1)).unwrap();
        player.next();
        // legato: note 2 takes over note 1's phase, now at wave[1], and
        // holds there because its own frequency is 0
        tx.push(on(2)).unwrap();
        assert_eq!(player.next(), 0.5);
        assert_eq!(player.next(), 0.5);
        assert_eq!(pitches(&player), vec![Some(2)]);
    }

    #[test]
    fn all_notes_off_releases_every_voice_on_its_instrument() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut wide = flat_instrument();
        wide.voice_count = 2;
        let instruments = vec![Arc::new(wide), Arc::new(flat_instrument())];
        let mut player = Player::live(instruments, freqs(), rx, 4.0);

        tx.push(on(1)).unwrap();
        tx.push(on(2)).unwrap();
        tx.push(Event::NoteOn {
            instrument: 1,
            note: 2,
            velocity: 127,
        })
        .unwrap();
        player.next();

        // flat_instrument has no release, so released voices free at once
        tx.push(Event::AllNotesOff { instrument: 0 }).unwrap();
        player.next();
        assert_eq!(pitches(&player), vec![None, None]);
        assert!(player.pools[1].voices[0].is_some(), "other instrument kept");
    }

    #[test]
    fn instruments_do_not_share_voices() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let instruments = vec![Arc::new(flat_instrument()), Arc::new(flat_instrument())];
        let mut player = Player::live(instruments, freqs(), rx, 4.0);

        tx.push(on(2)).unwrap();
        tx.push(Event::NoteOn {
            instrument: 1,
            note: 2,
            velocity: 127,
        })
        .unwrap();
        // each instrument is mono, but neither steals the other's voice
        assert_eq!(player.next(), 0.5);
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
