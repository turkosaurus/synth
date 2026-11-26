/// timbre and envelope
///
/// Built only through [`Instrument::new`], so holding one proves its wave
/// and envelope are safe to play.
pub struct Instrument {
    wave: Wavetable,
    /// 0.0 to 1.0
    attack: Segment,
    /// 1.0 to sustain_level
    decay: Segment,
    /// 0.0..1.0
    sustain_level: f32,
    /// sustain_level to 0.0
    release: Segment,
    /// notes sounding at once; 1 is mono
    voice_count: usize,
}

/// Why an instrument is not safe to play.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum InstrumentError {
    #[error("the oscillator has no waveform to play")]
    NoFrames,

    #[error("{} is {value} at phase {phase}; must be finite, -1 to 1", wave_name(*frame, *frames))]
    Wave {
        /// 0-based
        frame: usize,
        frames: usize,
        phase: f32,
        value: f32,
    },

    #[error("osc slopes[{}] is {value} at t = {t}; must be finite, 0 to 1", index + 1)]
    Slope {
        /// 0-based
        index: usize,
        t: f32,
        value: f32,
    },

    #[error("osc position is {value}; must be finite, 0 to 1")]
    Position { value: f32 },

    #[error("lfo wave is {value} at phase {phase}; must be finite, -1 to 1")]
    Lfo { phase: f32, value: f32 },

    #[error("lfo rate is {value} Hz; must be finite, at least 0")]
    LfoRate { value: f32 },

    #[error("lfo depth is {value}; must be finite, 0 to 1")]
    LfoDepth { value: f32 },

    #[error("envelope {stage} is {value} at t = {t}; must be finite, 0 to 1")]
    Envelope {
        stage: &'static str,
        t: f32,
        value: f32,
    },

    #[error("envelope {stage} is {ms} ms; must be finite, at least 0")]
    Time { stage: &'static str, ms: f64 },

    #[error("voice_count is 0; must be at least 1")]
    NoVoices,
}

/// A lone waveform, or `waves[k]` of a table, counted from 1.
fn wave_name(frame: usize, frames: usize) -> String {
    match frames {
        1 => "waveform".to_string(),
        _ => format!("waves[{}]", frame + 1),
    }
}

impl Instrument {
    pub fn new(
        wave: Wavetable,
        attack: Segment,
        decay: Segment,
        sustain_level: f32,
        release: Segment,
        voice_count: usize,
    ) -> Result<Self, InstrumentError> {
        let frames = wave.frame_count();
        if frames == 0 {
            return Err(InstrumentError::NoFrames);
        }
        for frame in 0..frames {
            let cycle = wave.frame(frame);
            if let Some((i, &value)) = outside(cycle, -1.0..=1.0) {
                return Err(InstrumentError::Wave {
                    frame,
                    frames,
                    phase: i as f32 / cycle.len() as f32,
                    value,
                });
            }
        }
        for (index, slope) in wave.slopes.iter().enumerate() {
            if let Some((i, &value)) = outside(slope, 0.0..=1.0) {
                return Err(InstrumentError::Slope {
                    index,
                    t: along(i, slope.len()),
                    value,
                });
            }
        }
        if outside(&[wave.position], 0.0..=1.0).is_some() {
            return Err(InstrumentError::Position {
                value: wave.position,
            });
        }
        if let Some(lfo) = &wave.position_mod {
            if let Some((i, &value)) = outside(&lfo.cycle, -1.0..=1.0) {
                return Err(InstrumentError::Lfo {
                    phase: i as f32 / lfo.cycle.len() as f32,
                    value,
                });
            }
            if !(lfo.rate.is_finite() && lfo.rate >= 0.0) {
                return Err(InstrumentError::LfoRate { value: lfo.rate });
            }
            // `position` is the only parameter an LFO moves, so 0 to 1
            if outside(&[lfo.depth], 0.0..=1.0).is_some() {
                return Err(InstrumentError::LfoDepth { value: lfo.depth });
            }
        }
        for (stage, segment) in [
            ("attack", &attack),
            ("decay", &decay),
            ("release", &release),
        ] {
            if let Some((i, &value)) = outside(&segment.curve, 0.0..=1.0) {
                return Err(InstrumentError::Envelope {
                    stage,
                    t: along(i, segment.curve.len()),
                    value,
                });
            }
            if !(segment.ms.is_finite() && segment.ms >= 0.0) {
                return Err(InstrumentError::Time {
                    stage,
                    ms: segment.ms,
                });
            }
        }
        if voice_count == 0 {
            return Err(InstrumentError::NoVoices);
        }
        Ok(Self {
            wave,
            attack,
            decay,
            sustain_level,
            release,
            voice_count,
        })
    }

    /// Skips every check, so tests can play what `new` refuses.
    #[cfg(test)]
    pub(crate) fn unchecked(
        wave: Wavetable,
        attack: Segment,
        decay: Segment,
        sustain_level: f32,
        release: Segment,
        voice_count: usize,
    ) -> Self {
        Self {
            wave,
            attack,
            decay,
            sustain_level,
            release,
            voice_count,
        }
    }

    pub fn wave(&self) -> &Wavetable {
        &self.wave
    }

    pub(crate) fn attack(&self) -> &Segment {
        &self.attack
    }

    pub(crate) fn decay(&self) -> &Segment {
        &self.decay
    }

    pub(crate) fn sustain_level(&self) -> f32 {
        self.sustain_level
    }

    pub(crate) fn release(&self) -> &Segment {
        &self.release
    }

    pub(crate) fn voice_count(&self) -> usize {
        self.voice_count
    }

    /// Rotates frame `k` left by `by` points. A rotation keeps every point,
    /// so the checks `new` made still hold.
    pub(crate) fn rotate_frame(&mut self, k: usize, by: usize) {
        self.wave.frame_mut(k).rotate_left(by);
    }
}

/// Where point `i` of a curve `len` long sits, 0 at the first and 1 at the
/// last.
fn along(i: usize, len: usize) -> f32 {
    i as f32 / len.saturating_sub(1).max(1) as f32
}

/// The first point that is not finite or falls outside `range`.
/// NaN fails `contains`, and so do both infinities at these bounds.
fn outside(points: &[f32], range: std::ops::RangeInclusive<f32>) -> Option<(usize, &f32)> {
    points.iter().enumerate().find(|(_, v)| !range.contains(v))
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
#[derive(Clone, Debug, PartialEq)]
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

    fn frame_mut(&mut self, k: usize) -> &mut [f32] {
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
#[non_exhaustive]
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
///
/// Built only through [`Freqs::new`], so every note has a pitch that is
/// positive and finite.
#[derive(Debug, Clone, PartialEq)]
pub struct Freqs([f32; 128]);

/// Why a tuning table is not safe to play.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum FreqsError {
    #[error("note {note} is {hz} Hz; must be positive and finite")]
    Hz { note: u8, hz: f32 },
}

impl Freqs {
    pub fn new(hz: [f32; 128]) -> Result<Self, FreqsError> {
        let bad = hz.iter().position(|v| !(v.is_finite() && *v > 0.0));
        if let Some(note) = bad {
            return Err(FreqsError::Hz {
                note: note as u8,
                hz: hz[note],
            });
        }
        Ok(Self(hz))
    }

    /// Skips every check, so tests can play what `new` refuses.
    #[cfg(test)]
    pub(crate) fn unchecked(hz: [f32; 128]) -> Self {
        Self(hz)
    }

    /// `None` above note 127.
    pub fn hz(&self, note: u8) -> Option<f32> {
        self.0.get(usize::from(note)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(curve: Vec<f32>) -> Segment {
        Segment { curve, ms: 1.0 }
    }

    fn build(wave: Wavetable, attack: Vec<f32>) -> Result<Instrument, InstrumentError> {
        Instrument::new(
            wave,
            stage(attack),
            stage(vec![0.0, 1.0]),
            0.5,
            stage(vec![0.0, 1.0]),
            1,
        )
    }

    fn table(frames: Vec<f32>, points: usize) -> Wavetable {
        Wavetable {
            frames,
            points,
            slopes: Vec::new(),
            position: 0.0,
            position_mod: None,
        }
    }

    #[test]
    fn a_clean_instrument_builds() {
        assert!(build(vec![0.0, 1.0, 0.0, -1.0].into(), vec![0.0, 1.0]).is_ok());
    }

    #[test]
    fn an_empty_wave_is_refused() {
        let err = build(Vec::new().into(), vec![0.0, 1.0]).err();
        assert_eq!(err, Some(InstrumentError::NoFrames));
        let err = build(table(Vec::new(), 4), vec![0.0, 1.0]).err();
        assert_eq!(err, Some(InstrumentError::NoFrames));
    }

    #[test]
    fn every_frame_is_checked() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.5, -1.01] {
            let wave = table(vec![0.0, 1.0, 0.0, -1.0, 0.0, 1.0, bad, -1.0], 4);
            let err = build(wave, vec![0.0, 1.0]).err().expect("refused");
            let InstrumentError::Wave { frame, phase, .. } = err else {
                panic!("{err:?}");
            };
            assert_eq!((frame, phase), (1, 0.5), "for {bad}");
        }
    }

    fn swept(slopes: Vec<Vec<f32>>, position: f32, cycle: Vec<f32>) -> Wavetable {
        Wavetable {
            slopes,
            position,
            position_mod: Some(Lfo {
                cycle,
                rate: 1.0,
                depth: 0.5,
            }),
            ..table(vec![0.0, 1.0, 0.0, -1.0], 2)
        }
    }

    #[test]
    fn slopes_position_and_lfo_are_checked() {
        let ok = || swept(vec![vec![0.0, 1.0]], 0.5, vec![0.0, 1.0]);
        assert!(build(ok(), vec![0.0, 1.0]).is_ok());
        for bad in [f32::NAN, f32::INFINITY, 1.5, -0.5] {
            let err = build(
                swept(vec![vec![0.0, 1.0], vec![0.0, bad, 1.0]], 0.5, vec![0.0]),
                vec![0.0, 1.0],
            )
            .err();
            assert!(
                matches!(err, Some(InstrumentError::Slope { index: 1, t, .. }) if t == 0.5),
                "{err:?}"
            );
            let err = build(swept(vec![], bad, vec![0.0]), vec![0.0, 1.0]).err();
            assert!(
                matches!(err, Some(InstrumentError::Position { .. })),
                "{err:?}"
            );
        }
        for bad in [f32::NAN, f32::NEG_INFINITY, 1.5] {
            let err = build(swept(vec![], 0.5, vec![0.0, bad]), vec![0.0, 1.0]).err();
            assert!(
                matches!(err, Some(InstrumentError::Lfo { phase, .. }) if phase == 0.5),
                "{err:?}"
            );
        }
    }

    fn lfo(rate: f32, depth: f32) -> Wavetable {
        Wavetable {
            position_mod: Some(Lfo {
                cycle: vec![0.0, 1.0],
                rate,
                depth,
            }),
            ..table(vec![0.0, 1.0, 0.0, -1.0], 2)
        }
    }

    #[test]
    fn lfo_rate_and_depth_are_checked() {
        assert!(build(lfo(0.0, 0.0), vec![0.0, 1.0]).is_ok());
        assert!(build(lfo(20.0, 1.0), vec![0.0, 1.0]).is_ok());
        for bad in [f32::NAN, f32::INFINITY, -0.1] {
            let err = build(lfo(bad, 0.5), vec![0.0, 1.0]).err();
            assert!(
                matches!(err, Some(InstrumentError::LfoRate { .. })),
                "{err:?} for {bad}"
            );
        }
        for bad in [f32::NAN, f32::INFINITY, -0.1, 1.5] {
            let err = build(lfo(1.0, bad), vec![0.0, 1.0]).err();
            assert!(
                matches!(err, Some(InstrumentError::LfoDepth { .. })),
                "{err:?} for {bad}"
            );
        }
    }

    #[test]
    fn every_time_is_checked() {
        let timed = |ms: [f64; 3]| {
            let segment = |ms| Segment {
                curve: vec![0.0, 1.0],
                ms,
            };
            Instrument::new(
                vec![0.0, 1.0].into(),
                segment(ms[0]),
                segment(ms[1]),
                0.5,
                segment(ms[2]),
                1,
            )
        };
        assert!(timed([0.0, 0.0, 0.0]).is_ok());
        for bad in [f64::NAN, f64::INFINITY, -1.0] {
            let err = timed([1.0, 1.0, bad]).err();
            assert!(
                matches!(
                    err,
                    Some(InstrumentError::Time {
                        stage: "release",
                        ..
                    })
                ),
                "{err:?} for {bad}"
            );
        }
    }

    #[test]
    fn no_voices_is_refused() {
        let err = Instrument::new(
            vec![0.0, 1.0].into(),
            stage(vec![0.0, 1.0]),
            stage(vec![0.0, 1.0]),
            0.5,
            stage(vec![0.0, 1.0]),
            0,
        )
        .err();
        assert_eq!(err, Some(InstrumentError::NoVoices));
    }

    #[test]
    fn every_note_needs_a_positive_finite_pitch() {
        let freqs = Freqs::new([440.0; 128]).expect("clean");
        assert_eq!(freqs.hz(127), Some(440.0));
        assert_eq!(freqs.hz(128), None);
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut hz = [440.0; 128];
            hz[60] = bad;
            let err = Freqs::new(hz).err();
            assert!(
                matches!(err, Some(FreqsError::Hz { note: 60, .. })),
                "{err:?} for {bad}"
            );
        }
    }

    #[test]
    fn every_envelope_point_is_checked() {
        for bad in [f32::NAN, f32::INFINITY, 1000.0, -0.1] {
            let err = build(vec![0.0, 1.0].into(), vec![0.0, bad, 1.0]).err();
            let Some(InstrumentError::Envelope { stage, t, .. }) = err else {
                panic!("{err:?} for {bad}");
            };
            assert_eq!((stage, t), ("attack", 0.5), "for {bad}");
        }
    }
}
