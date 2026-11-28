use std::sync::Arc;
use std::time::Duration;

use super::fx::{Lfo, Svf};
use crate::spec::{CutoffMod, Event, Filter, Freqs, Instrument, Timeline, Unit};

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

/// Attack, decay, and release in samples. The instrument holds each
/// stage's curve and length in ms; the rate is only known here.
struct Lengths {
    attack: usize,
    decay: usize,
    release: usize,
}

// one sounding note
/// One cycle at `phase`, read between the two entries either side.
fn cycle_at(wave: &[f32], phase: f32) -> f32 {
    let n = wave.len();
    let pos = phase * n as f32;
    let i = pos as usize % n;
    let j = (i + 1) % n;
    wave[i] + (wave[j] - wave[i]) * (pos - pos.floor())
}

struct Note {
    instrument: Arc<Instrument>,
    /// 0.0-1.0, scaling the envelope peak; differs from `target` only while
    /// easing after a steal
    velocity: f32,
    target: f32,
    glide: f32, // velocity moved per sample toward `target`
    ramp: f32,  // samples in GLIDE at this sample rate
    phase: f32, // 0.0..1.0
    /// the two frames either side of the table's position, and how far
    /// toward the second; set each sample while `lfo` moves `position`
    from: usize,
    to: usize,
    mix: f32,
    /// this note's own sweep, from the start of its cycle at note-on
    lfo: Option<Lfo>,
    step: f32, // phase increment per sample
    stage: Stage,
    /// each stage's length in samples at this note's rate
    lengths: Lengths,
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
        let (from, to, mix) = instrument.wave().blend();
        let lfo = instrument
            .wave()
            .position_mod
            .as_ref()
            .map(|baked| Lfo::new(baked.rate, sample_rate));
        Self {
            velocity,
            from,
            to,
            mix,
            lfo,
            target: velocity,
            glide: 0.0,
            ramp: (GLIDE.as_secs_f32() * sample_rate).max(1.0),
            step: freq / sample_rate,
            hold_samples: hold.map(|d| (d.as_secs_f32() * sample_rate) as usize),
            lengths: Lengths {
                attack: instrument.attack().samples(f64::from(sample_rate)),
                decay: instrument.decay().samples(f64::from(sample_rate)),
                release: instrument.release().samples(f64::from(sample_rate)),
            },
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
        self.release_scale = if self.instrument.sustain_level() > 0.0 {
            self.level / self.instrument.sustain_level()
        } else {
            1.0
        };
        self.stage = Stage::Release;
        self.idx = 0;
    }

    // the loop falls through zero-length segments, e.g. attack = 0
    fn envelope(&mut self) -> f32 {
        loop {
            match self.stage {
                Stage::Attack => {
                    if self.idx < self.lengths.attack {
                        let value = self.attack_at(self.idx);
                        self.idx += 1;
                        self.level = value;
                        return value;
                    }
                    self.stage = Stage::Decay;
                    self.idx = 0;
                }
                Stage::Decay => {
                    if self.idx < self.lengths.decay {
                        let value = self.decay_at(self.idx);
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
                    self.level = self.instrument.sustain_level();
                    return self.level;
                }
                Stage::Release => {
                    if self.idx < self.lengths.release {
                        let sustain = self.instrument.sustain_level();
                        let along = self
                            .instrument
                            .release()
                            .along(self.idx, self.lengths.release);
                        self.idx += 1;
                        self.level = (sustain - sustain * along) * self.release_scale;
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

    // attack rises from 0 to 1
    fn attack_at(&self, i: usize) -> f32 {
        self.instrument.attack().along(i, self.lengths.attack)
    }

    // decay falls from 1 to the sustain level
    fn decay_at(&self, i: usize) -> f32 {
        let along = self.instrument.decay().along(i, self.lengths.decay);
        1.0 + (self.instrument.sustain_level() - 1.0) * along
    }

    fn wave(&self) -> f32 {
        let table = self.instrument.wave();
        let a = cycle_at(table.frame(self.from), self.phase);
        // a single waveform, or a position on a frame, reads one frame
        if self.mix == 0.0 {
            return a;
        }
        let b = cycle_at(table.frame(self.to), self.phase);
        a + (b - a) * self.mix
    }

    fn next(&mut self) -> f32 {
        if let (Some(lfo), Some(baked)) = (&mut self.lfo, &self.instrument.wave().position_mod) {
            let table = self.instrument.wave();
            (self.from, self.to, self.mix) =
                table.blend_at(table.position + baked.depth * lfo.next(&baked.cycle));
        }
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
        self.idx = (0..self.lengths.attack)
            .find(|&i| self.attack_at(i) >= level)
            .unwrap_or(self.lengths.attack);

        let first = if self.idx < self.lengths.attack {
            self.attack_at(self.idx)
        } else if self.lengths.decay > 0 {
            self.decay_at(0)
        } else {
            self.instrument.sustain_level()
        };
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
        let voices = (0..instrument.voice_count()).map(|_| None).collect();
        let gain = 1.0 / (instrument.voice_count().max(1) as f32).sqrt();
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

    fn silence_all(&mut self) {
        self.voices.fill_with(|| None);
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

/// What a player plays and where its events come from. Downstream handling
/// is identical, so the sequencer and a live input differ only here.
pub enum Feed {
    Timeline(Timeline),
    Live {
        instruments: Vec<Instrument>,
        events: rtrb::Consumer<Event>,
    },
}

/// The reader's end of an output tee: output frames, `channels` samples
/// each, interleaved as the device takes them.
pub struct OutputTee {
    pub samples: rtrb::Consumer<f32>,
    pub channels: usize,
}

/// An output tee holding up to `frames` frames, and the producer for
/// [`Player::with_output_tee`].
pub fn output_tee(frames: usize, channels: usize) -> (rtrb::Producer<f32>, OutputTee) {
    let (tx, samples) = rtrb::RingBuffer::new(frames * channels);
    (tx, OutputTee { samples, channels })
}

/// A feed's events once its instruments are in pools.
enum Source {
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
    source: Source,
    sample_rate: f32,
    clock: u64,
    total_samples: u64,
    /// runs on the sum of every voice
    filter: Option<Svf>,
    /// moves `filter`'s cutoff each sample
    sweep: Option<Sweep>,
    /// receives every output frame, for a reader off the audio thread
    output_tee: Option<rtrb::Producer<f32>>,
}

/// One LFO moving the shared filter's cutoff from `base`. Never restarted,
/// since no single note owns a filter every voice runs through.
struct Sweep {
    lfo: Lfo,
    spec: CutoffMod,
    /// Hz, as the patch wrote it
    base: f32,
}

impl Sweep {
    fn drive(&mut self, filter: &mut Svf) {
        let along = self.spec.lfo.depth * self.lfo.next(&self.spec.lfo.cycle);
        match self.spec.unit {
            Unit::Hz => filter.set_cutoff(self.base + along),
            Unit::Octaves => filter.set_cutoff_octaves(self.base, along),
        }
    }
}

impl Player {
    pub fn new(feed: Feed, freqs: Freqs, sample_rate: f32) -> Self {
        let (instruments, source, total_samples) = match feed {
            Feed::Timeline(timeline) => (
                timeline.instruments,
                Source::Timeline {
                    events: timeline.events,
                    next: 0,
                },
                timeline.total_samples,
            ),
            Feed::Live {
                instruments,
                events,
            } => (instruments, Source::Live(events), u64::MAX),
        };
        Self {
            pools: instruments
                .into_iter()
                .map(|instrument| Pool::new(Arc::new(instrument)))
                .collect(),
            freqs,
            source,
            sample_rate,
            clock: 0,
            total_samples,
            filter: None,
            sweep: None,
            output_tee: None,
        }
    }

    /// Push every frame `fill` writes into `output_tee`, all channels
    /// interleaved. A full ring drops the frames that do not fit, so a
    /// stalled reader never holds up the callback.
    pub fn with_output_tee(mut self, output_tee: rtrb::Producer<f32>) -> Self {
        self.output_tee = Some(output_tee);
        self
    }

    /// Run the summed output through `filter`. Built here, at load, so
    /// the audio callback never allocates one.
    pub fn with_filter(mut self, filter: Option<Filter>) -> Self {
        let Some(mut filter) = filter else {
            self.filter = None;
            self.sweep = None;
            return self;
        };
        self.sweep = filter.cutoff_mod.take().map(|spec| Sweep {
            lfo: Lfo::new(spec.lfo.rate, self.sample_rate),
            spec,
            base: filter.cutoff,
        });
        self.filter = Some(Svf::new(filter, self.sample_rate));
        self
    }

    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    // returns an owned event so the borrow of `source` ends before `apply`
    // needs &mut self
    fn pull(&mut self) -> Option<Event> {
        let clock = self.clock;
        match &mut self.source {
            Source::Timeline { events, next } => {
                let (at, event) = events.get(*next)?;
                if *at <= clock {
                    *next += 1;
                    Some(*event)
                } else {
                    None
                }
            }
            Source::Live(consumer) => consumer.pop().ok(),
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
                let Some(freq) = self.freqs.hz(note) else {
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
            Event::AllSoundOff { instrument } => {
                if let Some(pool) = self.pools.get_mut(instrument) {
                    pool.silence_all();
                }
            }
        }
    }

    fn release(&mut self, instrument: usize, note: u8) {
        if let Some(pool) = self.pools.get_mut(instrument) {
            pool.release(note);
        }
    }

    /// Applies every event due now: a timeline's stamped at or before the
    /// clock, or everything waiting in a live ring. Returns how many.
    fn apply_due(&mut self) -> u32 {
        let mut applied = 0;
        // several events can land on the same sample, so drain rather than peek
        while let Some(event) = self.pull() {
            self.apply(event);
            applied += 1;
        }
        applied
    }

    /// Samples that can be rendered before the next event is due. Live
    /// events carry no time stamp, so a live feed is read once per callback
    /// and never splits one.
    fn until_event(&self) -> u64 {
        match &self.source {
            Source::Timeline { events, next } => events
                .get(*next)
                .map_or(u64::MAX, |(at, _)| at.saturating_sub(self.clock)),
            Source::Live(_) => u64::MAX,
        }
    }

    /// One sample as a one-frame callback would play it.
    #[cfg(test)]
    fn next(&mut self) -> f32 {
        self.apply_due();
        self.render()
    }

    /// One sample, without reading the feed.
    fn render(&mut self) -> f32 {
        self.clock += 1;
        let sum: f32 = self.pools.iter_mut().map(Pool::next).sum();
        match &mut self.filter {
            Some(filter) => {
                if let Some(sweep) = &mut self.sweep {
                    sweep.drive(filter);
                }
                filter.next(sum)
            }
            None => sum,
        }
    }

    // the filter is the only state that feeds a sample back into the next,
    // so it is the only place a NaN can persist
    fn reset_filter(&mut self) {
        if let Some(filter) = &mut self.filter {
            filter.reset();
        }
    }
}

/// What one callback produced. Accumulated in locals so the atomics are
/// touched once per callback instead of once per sample.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Tally {
    pub frames: u32,
    /// samples beyond full scale, each written clamped to it
    pub clipped: u64,
    /// samples that were NaN or infinite, each written as silence
    pub non_finite: u64,
    /// Largest finite magnitude, taken before the clamp so a meter shows how
    /// far over full scale the output went. Non-finite samples are left out:
    /// an infinity would pin the meter, and `Stats::record_peak` orders bit
    /// patterns, which only holds for non-negative finite floats.
    pub peak: f32,
    /// frames the output tee had no room for
    pub output_tee_dropped: u64,
    /// events applied
    pub events: u32,
}

/// Full scale. A sample beyond this cannot be represented, so `fill`
/// clamps it here.
///
/// A baked wavetable stays within it, but an envelope above 1.0,
/// summed voices, or a resonant filter can each push a sample past it.
const CLIP: f32 = 1.0;

/// Lay one sample per frame across every channel. Split out of the callback
/// so the interleaving can be tested without an audio device.
///
/// This is the last step before the device, so it bounds what goes out. A
/// sample beyond full scale is clamped to it. A NaN or infinity is written
/// as silence and resets the filter, whose state would otherwise carry the
/// NaN into every sample after it.
///
/// The feed is read where an event can land: once at the start, which
/// takes everything a live ring holds, then at each timeline event's
/// sample. The spans between render without looking at it.
///
/// Once the buffer is written, the player's output tee gets a copy of it.
pub(super) fn fill(out: &mut [f32], channels: usize, player: &mut Player) -> Tally {
    let mut tally = render(out, channels, player);
    if let Some(output_tee) = &mut player.output_tee {
        tally.output_tee_dropped = push(output_tee, out, channels);
    }
    tally
}

fn render(out: &mut [f32], channels: usize, player: &mut Player) -> Tally {
    let mut tally = Tally::default();
    let mut frames = out.chunks_mut(channels);
    loop {
        tally.events += player.apply_due();
        let span = usize::try_from(player.until_event()).unwrap_or(usize::MAX);
        let mut written = 0;
        for frame in frames.by_ref().take(span) {
            write(frame, player, &mut tally);
            written += 1;
        }
        if written < span {
            return tally;
        }
    }
}

/// Copies whole frames of `out` into `output_tee`, as many as it has room
/// for, and returns how many frames it had no room for. Writing only whole
/// frames keeps the reader's channels aligned whatever the ring's size.
fn push(output_tee: &mut rtrb::Producer<f32>, out: &[f32], channels: usize) -> u64 {
    let frames = out.len() / channels;
    let room = frames.min(output_tee.slots() / channels);
    if let Ok(chunk) = output_tee.write_chunk_uninit(room * channels) {
        chunk.fill_from_iter(out.iter().copied());
    }
    (frames - room) as u64
}

/// Renders one sample and lays it across `frame`, bounded as `fill` says.
fn write(frame: &mut [f32], player: &mut Player, tally: &mut Tally) {
    let raw = player.render();
    let sample = if raw.is_finite() {
        if raw.abs() > CLIP {
            tally.clipped += 1;
        }
        tally.peak = tally.peak.max(raw.abs());
        raw.clamp(-CLIP, CLIP)
    } else {
        tally.non_finite += 1;
        player.reset_filter();
        0.0
    };
    tally.frames += 1;
    for channel in frame.iter_mut() {
        *channel = sample;
    }
}

#[cfg(test)]
mod tests;
