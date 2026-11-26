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
        }
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
    /// clock, or everything waiting in a live ring.
    fn apply_due(&mut self) {
        // several events can land on the same sample, so drain rather than peek
        while let Some(event) = self.pull() {
            self.apply(event);
        }
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
    pub clips: u64,
    /// samples that were NaN or infinite, each written as silence
    pub non_finite: u64,
    /// Largest finite magnitude, taken before the clamp so a meter shows how
    /// far over full scale the output went. Non-finite samples are left out:
    /// an infinity would pin the meter, and `Stats::record_peak` orders bit
    /// patterns, which only holds for non-negative finite floats.
    pub peak: f32,
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
pub(super) fn fill(out: &mut [f32], channels: usize, player: &mut Player) -> Tally {
    let mut tally = Tally::default();
    let mut frames = out.chunks_mut(channels);
    loop {
        player.apply_due();
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

/// Renders one sample and lays it across `frame`, bounded as `fill` says.
fn write(frame: &mut [f32], player: &mut Player, tally: &mut Tally) {
    let raw = player.render();
    let sample = if raw.is_finite() {
        if raw.abs() > CLIP {
            tally.clips += 1;
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
mod tests {
    use super::*;
    use crate::spec::{Kind, Lfo, Segment, Wavetable};

    /// A stage that plays `along` one point per sample at the 4 Hz test
    /// rate, so each sample reads a point exactly.
    fn stage(along: &[f32]) -> Segment {
        let mut curve = along.to_vec();
        curve.push(1.0);
        Segment {
            curve,
            ms: along.len() as f64 * 250.0,
        }
    }

    /// What `Instrument::new` takes, so a test can change one part.
    struct Parts {
        wave: Wavetable,
        attack: Segment,
        decay: Segment,
        sustain_level: f32,
        release: Segment,
        voice_count: usize,
    }

    impl Parts {
        fn build(self) -> Instrument {
            Instrument::new(
                self.wave,
                self.attack,
                self.decay,
                self.sustain_level,
                self.release,
                self.voice_count,
            )
            .unwrap()
        }

        fn unchecked(self) -> Instrument {
            Instrument::unchecked(
                self.wave,
                self.attack,
                self.decay,
                self.sustain_level,
                self.release,
                self.voice_count,
            )
        }
    }

    // 4-entry wave so interpolation is checkable by hand
    fn parts() -> Parts {
        Parts {
            wave: vec![0.0, 1.0, 0.0, -1.0].into(),
            attack: stage(&[0.0, 0.5]),
            decay: stage(&[0.0, 0.5]), // 1.0, 0.85
            sustain_level: 0.7,
            release: stage(&[0.0, 0.5]), // 0.7, 0.35
            voice_count: 1,
        }
    }

    fn instrument() -> Instrument {
        parts().build()
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

    /// Frames that each hold one level, so a read at any phase shows the
    /// blend directly.
    fn table(levels: &[f32], slopes: Vec<Vec<f32>>, position: f32) -> Instrument {
        table_parts(levels, slopes, position).build()
    }

    fn table_parts(levels: &[f32], slopes: Vec<Vec<f32>>, position: f32) -> Parts {
        Parts {
            wave: Wavetable {
                frames: levels.iter().flat_map(|&level| [level; 4]).collect(),
                points: 4,
                slopes,
                position,
                position_mod: None,
            },
            ..parts()
        }
    }

    /// Two frames a blend apart, swept by an LFO whose cycle reads
    /// 0, 0.5, 1, -1 at one entry per sample.
    fn swept(voice_count: usize) -> Pool {
        let mut parts = table_parts(&[0.0, 1.0], vec![], 0.5);
        parts.voice_count = voice_count;
        parts.wave.position_mod = Some(Lfo {
            cycle: vec![0.0, 0.5, 1.0, -1.0],
            rate: 1.0,
            depth: 0.5,
        });
        Pool::new(Arc::new(parts.build()))
    }

    fn start(pool: &mut Pool, slot: usize, pitch: u8) {
        pool.voices[slot] = Some(Voice {
            note: Note::new(Arc::clone(&pool.instrument), 1.0, 1.0, 4.0, None),
            pitch,
            age: 0,
        });
    }

    fn mix(pool: &Pool, slot: usize) -> f32 {
        pool.voices[slot].as_ref().unwrap().note.mix
    }

    #[test]
    fn an_lfo_moves_a_note_through_the_table() {
        let mut pool = swept(1);
        start(&mut pool, 0, 60);
        // position 0.5 + 0.5 × (0, 0.5, 1): the blend follows each sample
        let got: Vec<f32> = (0..3)
            .map(|_| {
                pool.next();
                mix(&pool, 0)
            })
            .collect();
        assert_eq!(got, [0.5, 0.75, 1.0]);
    }

    #[test]
    fn every_note_starts_its_own_sweep() {
        let mut pool = swept(2);
        start(&mut pool, 0, 60);
        pool.next();
        pool.next();
        // the second note starts two samples after the first
        start(&mut pool, 1, 64);
        pool.next();
        assert_eq!(
            mix(&pool, 1),
            0.5,
            "the new note is at the start of its sweep"
        );
        assert_eq!(mix(&pool, 0), 1.0, "the first note kept its own place");
    }

    #[test]
    fn a_table_sounds_its_end_frames_at_position_0_and_1() {
        assert_eq!(note(table(&[0.2, 0.8], vec![], 0.0)).wave(), 0.2);
        assert_eq!(note(table(&[0.2, 0.8], vec![], 1.0)).wave(), 0.8);
    }

    #[test]
    fn a_table_blends_linearly_with_no_slopes() {
        let level = note(table(&[0.2, 0.8], vec![], 0.5)).wave();
        assert!((level - 0.5).abs() < 1e-6, "{level}");
    }

    #[test]
    fn a_slope_shapes_the_blend() {
        // t squared at three points: halfway along is a quarter of the way
        let level = note(table(&[0.0, 1.0], vec![vec![0.0, 0.25, 1.0]], 0.5)).wave();
        assert!((level - 0.25).abs() < 1e-6, "{level}");
    }

    #[test]
    fn a_position_picks_the_transition_it_falls_in() {
        // three frames make two transitions; 0.75 is halfway through the second
        let level = note(table(&[0.0, 0.4, 1.0], vec![], 0.75)).wave();
        assert!((level - 0.7).abs() < 1e-6, "{level}");
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
    fn a_stage_longer_than_its_curve_reads_between_points() {
        // two points stretched over 1 s, 4 samples at 4 Hz
        let instrument = Parts {
            attack: Segment {
                curve: vec![0.0, 1.0],
                ms: 1000.0,
            },
            ..parts()
        };
        let mut note = note(instrument.build());
        let got: Vec<f32> = (0..4).map(|_| note.envelope()).collect();
        assert_eq!(got, [0.0, 0.25, 0.5, 0.75]);
    }

    #[test]
    fn zero_length_segment_falls_through() {
        let instrument = Parts {
            attack: stage(&[]),
            ..parts()
        };
        // with no attack samples the first value must come from decay
        assert_eq!(note(instrument.build()).envelope(), 1.0);
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
    fn flat() -> Parts {
        Parts {
            wave: vec![0.25, 0.5, 0.75, 1.0].into(),
            attack: stage(&[]),
            decay: stage(&[]),
            sustain_level: 1.0,
            release: stage(&[]),
            voice_count: 1,
        }
    }

    fn flat_instrument() -> Instrument {
        flat().build()
    }

    // a Player whose timeline is a single note-on at sample 0
    // freqs[1] = 1.0 so step is 1/4 of the table per sample at rate 4.0
    fn freqs() -> Freqs {
        Freqs::unchecked(hz())
    }

    fn hz() -> [f32; 128] {
        let mut hz = [0.0; 128];
        hz[1] = 1.0;
        hz
    }

    fn live(instruments: Vec<Instrument>, events: rtrb::Consumer<Event>) -> Feed {
        Feed::Live {
            instruments,
            events,
        }
    }

    fn player(instrument: Instrument) -> Player {
        Player::new(
            Feed::Timeline(Timeline {
                instruments: vec![instrument],
                events: vec![(
                    0,
                    Event::NoteOn {
                        instrument: 0,
                        note: 1,
                        velocity: 127,
                    },
                )],
                total_samples: 64,
            }),
            freqs(),
            4.0,
        )
    }

    // the flat instrument's wave sits above zero, so a highpass, which
    // removes that offset, must leave the output quieter on average
    #[test]
    fn filter_shapes_player_output() {
        let level = |filter: Option<Filter>| {
            let mut player = player(flat_instrument()).with_filter(filter);
            let out: Vec<f32> = (0..64).map(|_| player.next()).collect();
            out[32..].iter().map(|y| y.abs()).sum::<f32>()
        };
        let highpass = Filter {
            kind: Kind::Highpass,
            cutoff: 20000.0, // clamps just below Nyquist
            damping: core::f32::consts::SQRT_2,
            root: 1.0,
            cutoff_mod: None,
        };
        let (dry, wet) = (level(None), level(Some(highpass)));
        assert!(wet < dry, "filtered {wet} is not below unfiltered {dry}");
    }

    /// A live player at 48 kHz whose lowpass at 1000 Hz is moved by an
    /// LFO with `cycle`, at one entry per sample.
    fn swept_filter(cycle: Vec<f32>, depth: f32, unit: Unit) -> (rtrb::Producer<Event>, Player) {
        let (tx, rx) = rtrb::RingBuffer::new(4);
        let lowpass = Filter {
            kind: Kind::Lowpass,
            cutoff: 1000.0,
            damping: core::f32::consts::SQRT_2,
            root: 1.0,
            cutoff_mod: Some(CutoffMod {
                lfo: Lfo {
                    rate: 48000.0 / cycle.len() as f32,
                    cycle,
                    depth,
                },
                unit,
            }),
        };
        let player = Player::new(live(vec![flat_instrument()], rx), freqs(), 48000.0)
            .with_filter(Some(lowpass));
        (tx, player)
    }

    fn cutoff(player: &Player) -> f32 {
        player.filter.as_ref().unwrap().cutoff()
    }

    #[test]
    fn cutoff_mod_moves_the_cutoff_in_its_unit_and_glides() {
        for (cycle, depth, unit, want) in [
            (vec![-1.0, -1.0], 300.0, Unit::Hz, 700.0),
            (vec![1.0, 1.0], 1.0, Unit::Octaves, 2000.0),
        ] {
            let (_tx, mut player) = swept_filter(cycle, depth, unit);
            player.next();
            let first = cutoff(&player);
            assert!((first - 1000.0).abs() < 50.0, "{unit:?} jumped to {first}");
            for _ in 0..48000 {
                player.next();
            }
            let settled = cutoff(&player);
            assert!(
                (settled - want).abs() < 1.0,
                "{unit:?} settled at {settled}"
            );
        }
    }

    #[test]
    fn a_note_on_does_not_restart_the_cutoff_lfo() {
        let cycle = vec![0.0, 0.5, 1.0, 0.5, 0.0, -0.5, -1.0, -0.5];
        let (_quiet_tx, mut quiet) = swept_filter(cycle.clone(), 1.0, Unit::Octaves);
        let (mut tx, mut played) = swept_filter(cycle, 1.0, Unit::Octaves);
        for i in 0..64 {
            if i == 13 {
                tx.push(on(1)).unwrap();
            }
            quiet.next();
            played.next();
            assert_eq!(cutoff(&quiet), cutoff(&played), "sample {i}");
        }
    }

    #[test]
    fn a_live_feed_drains_the_queue() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);

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
        let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);

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
        let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);

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
        let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);

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
        Parts {
            attack: stage(&[0.0, 0.25, 0.5, 0.75]),
            ..parts()
        }
        .build()
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
        let instrument = Parts {
            attack: stage(&[]),
            ..parts()
        }
        .build();
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
        let instrument = Parts {
            voice_count,
            ..flat()
        };
        (
            tx,
            Player::new(live(vec![instrument.build()], rx), freqs(), 4.0),
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
        let instrument = Parts {
            release: stage(&[0.0, 0.5, 0.75]), // 1.0, 0.5, 0.25
            ..flat()
        };
        let mut player = Player::new(live(vec![instrument.build()], rx), freqs(), 4.0);

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
        let instrument = Parts {
            release: stage(&[0.0, 0.5, 0.75]), // 1.0, 0.5, 0.25
            ..flat()
        };
        let mut player = Player::new(live(vec![instrument.build()], rx), freqs(), 4.0);

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
        let instrument = Parts {
            voice_count: 2,
            release: stage(&[0.0; 16]), // long enough to still be fading
            ..flat()
        };
        let mut player = Player::new(live(vec![instrument.build()], rx), freqs(), 4.0);

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
        let wide = Parts {
            voice_count: 2,
            ..flat()
        }
        .build();
        let instruments = vec![wide, flat_instrument()];
        let mut player = Player::new(live(instruments, rx), freqs(), 4.0);

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

    // a release stage that All Notes Off would play, so silence on the next
    // sample shows it was skipped
    #[test]
    fn all_sound_off_silences_its_instrument_with_no_release() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let releasing = Parts {
            release: stage(&[0.0, 0.5, 0.75]),
            ..flat()
        }
        .build();
        let instruments = vec![releasing, flat_instrument()];
        let mut player = Player::new(live(instruments, rx), freqs(), 4.0);

        tx.push(on(1)).unwrap();
        tx.push(Event::NoteOn {
            instrument: 1,
            note: 2,
            velocity: 127,
        })
        .unwrap();
        player.next();

        tx.push(Event::AllSoundOff { instrument: 0 }).unwrap();
        player.next();
        assert_eq!(pitches(&player), vec![None]);
        assert!(player.pools[1].voices[0].is_some(), "other instrument kept");
    }

    #[test]
    fn instruments_do_not_share_voices() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let instruments = vec![flat_instrument(), flat_instrument()];
        let mut player = Player::new(live(instruments, rx), freqs(), 4.0);

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

    // splitting the buffer at each event must sound the same as reading the
    // feed before every sample: events mid-buffer, two on one sample, one on
    // a buffer's first sample, and one on its last
    #[test]
    fn fill_splits_the_buffer_at_each_timeline_event() {
        let on = |note| Event::NoteOn {
            instrument: 0,
            note,
            velocity: 127,
        };
        let off = |note| Event::NoteOff {
            instrument: 0,
            note,
        };
        let timeline = || Timeline {
            instruments: vec![flat_instrument()],
            events: vec![(1, on(1)), (3, off(1)), (3, on(2)), (5, off(2)), (8, on(1))],
            total_samples: 12,
        };
        let mut hz = hz();
        hz[2] = 2.0;
        let freqs = Freqs::unchecked(hz);

        let mut stepped = Player::new(Feed::Timeline(timeline()), freqs.clone(), 4.0);
        let expected: Vec<f32> = (0..12).map(|_| stepped.next()).collect();

        let mut split = Player::new(Feed::Timeline(timeline()), freqs, 4.0);
        let mut out = Vec::new();
        for frames in [5, 1, 3, 3] {
            let mut buffer = vec![9.0; frames];
            fill(&mut buffer, 1, &mut split);
            out.extend(buffer);
        }
        assert_eq!(out, expected);
    }

    // live events carry no time stamp, so all of them land on a callback's
    // first frame
    #[test]
    fn fill_plays_live_events_from_the_first_frame() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let mut player = Player::new(live(vec![flat_instrument()], rx), freqs(), 4.0);
        tx.push(Event::NoteOn {
            instrument: 0,
            note: 1,
            velocity: 127,
        })
        .unwrap();

        let mut out = [0.0f32; 4];
        fill(&mut out, 1, &mut player);
        assert_eq!(out, [0.25, 0.5, 0.75, 1.0]);
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
        let instrument = Parts {
            wave: vec![0.5, -2.0, 1.5, 0.1].into(),
            ..flat()
        }
        .unchecked();

        let mut out = [0.0f32; 8];
        let tally = fill(&mut out, 2, &mut player(instrument));

        assert_eq!(tally.frames, 4);
        // -2.0 and 1.5 are written at full scale
        assert_eq!(out, [0.5, 0.5, -1.0, -1.0, 1.0, 1.0, 0.1, 0.1]);
        assert_eq!(tally.clips, 2); // -2.0 and 1.5 both exceed full scale
        assert_eq!(tally.peak, 2.0); // absolute and pre-clamp, so -2.0 wins
    }

    #[test]
    fn fill_drops_non_finite_samples() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let instrument = Parts {
                wave: vec![0.5, bad, 0.25, 0.75].into(),
                ..flat()
            }
            .unchecked();

            let mut out = [0.0f32; 4];
            let tally = fill(&mut out, 1, &mut player(instrument));

            // wave[0] interpolates toward wave[1] with a weight of zero, and
            // a non-finite value times zero is NaN, so two samples are bad
            assert_eq!(out, [0.0, 0.0, 0.25, 0.75], "for {bad}");
            assert_eq!(tally.non_finite, 2, "for {bad}");
            assert_eq!(tally.clips, 0, "for {bad}");
            assert_eq!(tally.peak, 0.75, "for {bad}");
        }
    }

    // Without the reset the filter state stays NaN, so every later sample
    // would be dropped as well.
    #[test]
    fn the_filter_recovers_after_a_non_finite_sample() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let broken = Parts {
            wave: vec![0.5, f32::NAN, 0.25, 0.75].into(),
            ..flat()
        }
        .unchecked();
        let lowpass = Filter {
            kind: Kind::Lowpass,
            cutoff: 20000.0, // clamps just below Nyquist
            damping: core::f32::consts::SQRT_2,
            root: 1.0,
            cutoff_mod: None,
        };
        let instruments = vec![broken, flat_instrument()];
        let mut player =
            Player::new(live(instruments, rx), freqs(), 4.0).with_filter(Some(lowpass));

        tx.push(on(1)).unwrap();
        let mut out = [0.0f32; 4];
        assert_eq!(fill(&mut out, 1, &mut player).non_finite, 2);

        // the broken note reads NaN once more as it ends, and after that only
        // the clean instrument sounds
        tx.push(off(1)).unwrap();
        tx.push(Event::NoteOn {
            instrument: 1,
            note: 1,
            velocity: 127,
        })
        .unwrap();
        let mut out = [0.0f32; 8];
        let tally = fill(&mut out, 1, &mut player);

        assert_eq!(tally.non_finite, 1);
        assert_eq!(out[0], 0.0);
        for &y in &out[1..] {
            assert!(y.is_finite() && y != 0.0 && y.abs() <= CLIP, "{out:?}");
        }
    }

    #[test]
    fn fill_clamps_summed_voices_to_full_scale() {
        let (mut tx, rx) = rtrb::RingBuffer::<Event>::new(8);
        let loud = || {
            Parts {
                wave: vec![1.0, -1.0, 1.0, -1.0].into(),
                ..flat()
            }
            .build()
        };
        let mut player = Player::new(live(vec![loud(), loud()], rx), freqs(), 4.0);

        // two mono instruments add with no scaling, so every sample is 2.0
        // or -2.0 before the clamp
        tx.push(on(1)).unwrap();
        tx.push(Event::NoteOn {
            instrument: 1,
            note: 1,
            velocity: 127,
        })
        .unwrap();
        let mut out = [0.0f32; 4];
        let tally = fill(&mut out, 1, &mut player);

        assert_eq!(out, [1.0, -1.0, 1.0, -1.0]);
        assert_eq!(tally.clips, 4);
        assert_eq!(tally.peak, 2.0);
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
