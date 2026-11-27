use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use super::fault::{Fault, Faults, StreamKind};
use super::render::{fill, Player, Tally};
use super::timing::Recorder;

/// A running output stream; audio stops when it drops. Named here so
/// holders need not import cpal, which stays in this file.
pub use cpal::Stream;

// written by the audio thread, except `dropped`, and read anywhere.
// lock-free because the callback must never block, and Relaxed because
// these are standalone counters, not gates on other memory.
#[derive(Default)]
pub struct Stats {
    /// frames, not samples: one frame is one sample per channel
    frames: AtomicU64,
    clipped: AtomicU64,
    /// NaN or infinite samples, each written as silence
    non_finite: AtomicU64,
    /// callbacks that ran past their deadline, which is the count that
    /// corresponds to something audible
    overruns: AtomicU64,
    /// errors the driver reported; each kind goes to a fault ring as well
    errors: AtomicU64,
    /// events the UI thread could not push because the ring was full
    dropped: AtomicU64,
    /// output frames the callback could not push to the output tee because
    /// its ring was full
    output_tee_dropped: AtomicU64,
    /// timing records the callback could not push because their ring was full
    timing_dropped: AtomicU64,
    peak_bits: AtomicU32,
    /// the most recent callback's deadline, so it describes a callback that
    /// actually ran
    budget_us: AtomicU32,
    /// the most recent callback's frames, which the driver may vary
    buffer_frames: AtomicU32,
    /// the most recent callback's `playback - callback`
    output_us: AtomicU32,
    callback_us_max: AtomicU32,
    load_bits: AtomicU32,
}

impl Stats {
    // for non-negative floats the IEEE-754 bit pattern orders the same as
    // the value, so fetch_max on the bits is a correct max on the float
    fn record_peak(&self, peak: f32) {
        self.peak_bits
            .fetch_max(peak.abs().to_bits(), Ordering::Relaxed);
    }

    fn record_load(&self, load: f32) {
        self.load_bits.fetch_max(load.to_bits(), Ordering::Relaxed);
    }

    // a gauge like the others: a lifetime max would pin at the loudest
    // moment of the session and never fall, which no meter can read
    fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak_bits.swap(0, Ordering::Relaxed))
    }

    pub(crate) fn record_dropped(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// How long one callback has before the buffer it was handed is late.
    /// Polyphony needs this as a number to know what it can afford, and
    /// asking must not disturb the gauges, so this does not reset.
    pub(crate) fn budget_us(&self) -> u32 {
        self.budget_us.load(Ordering::Relaxed)
    }

    /// Read every counter once. The gauges reset here, so a snapshot
    /// describes the period since the previous one; a lifetime max would be
    /// pinned by the first callback after startup and could never report an
    /// improvement. Taking one reading in one call is what keeps that
    /// resetting correct, since a caller cannot read a gauge twice.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            frames: self.frames.load(Ordering::Relaxed),
            clipped: self.clipped.load(Ordering::Relaxed),
            non_finite: self.non_finite.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            output_tee_dropped: self.output_tee_dropped.load(Ordering::Relaxed),
            timing_dropped: self.timing_dropped.load(Ordering::Relaxed),
            peak: self.take_peak(),
            budget_us: self.budget_us(),
            buffer_frames: self.buffer_frames.load(Ordering::Relaxed),
            output_us: self.output_us.load(Ordering::Relaxed),
            callback_us_max: self.callback_us_max.swap(0, Ordering::Relaxed),
            load: f32::from_bits(self.load_bits.swap(0, Ordering::Relaxed)),
        }
    }
}

/// One reading of [`Stats`]. Plain data, so a view can render it and a log
/// can carry it as fields without either touching an atomic.
#[derive(Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Snapshot {
    pub frames: u64,
    pub clipped: u64,
    pub non_finite: u64,
    pub overruns: u64,
    pub errors: u64,
    pub dropped: u64,
    pub output_tee_dropped: u64,
    pub timing_dropped: u64,
    pub peak: f32,
    pub budget_us: u32,
    /// frames the most recent callback was handed; 0 before the first
    pub buffer_frames: u32,
    /// the most recent callback's `playback - callback`, as the driver
    /// reports it
    pub output_us: u32,
    pub callback_us_max: u32,
    /// worst fraction of the deadline used, 1.0 being the deadline itself
    pub load: f32,
}

/// Why the output device could not be opened or streamed to. The driver's
/// own error rides along as the source where there is one.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("no default output device")]
    NoDevice,

    #[error("reading the output device's default config")]
    Config {
        #[source]
        source: cpal::Error,
    },

    #[error("device wants {format:?}; f32, i16, and i32 are wired up")]
    Format { format: cpal::SampleFormat },

    #[error("device has {channels} channels; at most {max} are wired up")]
    Channels { channels: u16, max: usize },

    #[error("config.lua: `buffer_frames` is {frames}; this device takes {min} to {max}")]
    BufferFrames { frames: u32, min: u32, max: u32 },

    #[error("opening the output stream ({buffer:?})")]
    Stream {
        buffer: cpal::BufferSize,
        #[source]
        source: cpal::Error,
    },

    #[error("starting the output stream")]
    Play {
        #[source]
        source: cpal::Error,
    },
}

/// A negotiated output device, ready to build a stream on.
pub struct Output {
    pub(crate) device: cpal::Device,
    pub(crate) config: cpal::SupportedStreamConfig,
    pub(crate) buffer: cpal::BufferSize,
    pub sample_rate: u32,
    pub channels: usize,
}

impl Output {
    /// The frames per callback asked for; `None` left it to the platform.
    pub fn requested(&self) -> Option<u32> {
        match self.buffer {
            cpal::BufferSize::Fixed(frames) => Some(frames),
            cpal::BufferSize::Default => None,
        }
    }
}

/// Open the default output device, asking for `buffer_frames` per callback
/// if given. Separated from playback so every view and command negotiates
/// the device the same way.
pub fn open(buffer_frames: Option<u32>) -> Result<Output, Error> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(Error::NoDevice)?;
    let config = device
        .default_output_config()
        .map_err(|source| Error::Config { source })?;
    let format = config.sample_format();
    if !matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::I32
    ) {
        return Err(Error::Format { format });
    }
    if usize::from(config.channels()) > SCRATCH {
        return Err(Error::Channels {
            channels: config.channels(),
            max: SCRATCH,
        });
    }

    let buffer = buffer_size(buffer_frames, config.buffer_size())?;

    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    debug!(sample_rate, channels, format = ?config.sample_format(), ?buffer, "device");

    Ok(Output {
        device,
        config,
        buffer,
        sample_rate,
        channels,
    })
}

/// The buffer to ask for. A size outside the range the device reports is
/// named here, rather than failing later inside the driver. A device that
/// reports no range gets the request as written.
fn buffer_size(
    frames: Option<u32>,
    supported: &cpal::SupportedBufferSize,
) -> Result<cpal::BufferSize, Error> {
    let Some(frames) = frames else {
        return Ok(cpal::BufferSize::Default);
    };
    if let cpal::SupportedBufferSize::Range { min, max } = *supported {
        if !(min..=max).contains(&frames) {
            return Err(Error::BufferFrames { frames, min, max });
        }
    }
    Ok(cpal::BufferSize::Fixed(frames))
}

/// Build and start a stream that drives `player`, reporting into `stats`
/// and `faults`, and into `recorder` when one is given.
pub fn stream(
    output: &Output,
    player: Player,
    stats: Arc<Stats>,
    recorder: Option<Recorder>,
    faults: &mut Faults,
) -> Result<cpal::Stream, Error> {
    // cpal may call the error handler on a thread other than the
    // callback's, so each gets its own ring
    let rings = (faults.ring(), faults.ring());
    match output.config.sample_format() {
        SampleFormat::I16 => build::<i16>(output, player, stats, recorder, rings, convert),
        SampleFormat::I32 => build::<i32>(output, player, stats, recorder, rings, convert),
        _ => build::<f32>(output, player, stats, recorder, rings, direct),
    }
}

/// Told how many events each render pass applied.
type Applied<'a> = dyn FnMut(u32) + 'a;

/// Renders straight into an f32 device buffer in one pass.
fn direct(out: &mut [f32], channels: usize, player: &mut Player, applied: &mut Applied) -> Tally {
    let tally = fill(out, channels, player);
    applied(tally.events);
    tally
}

/// Samples rendered per pass when the device is not f32: enough to keep
/// the passes few, small enough to live on the callback's stack.
const SCRATCH: usize = 1024;

/// Renders into a stack buffer and converts each sample to `T`, in passes
/// of whole frames so `fill` never sees a frame split across two of them.
fn convert<T>(out: &mut [T], channels: usize, player: &mut Player, applied: &mut Applied) -> Tally
where
    T: Sample + FromSample<f32>,
{
    let mut scratch = [0.0f32; SCRATCH];
    let pass = SCRATCH / channels * channels;
    let mut tally = Tally::default();
    for chunk in out.chunks_mut(pass) {
        let rendered = &mut scratch[..chunk.len()];
        let part = fill(rendered, channels, player);
        applied(part.events);
        for (sample, &value) in chunk.iter_mut().zip(rendered.iter()) {
            *sample = T::from_sample(value);
        }
        tally.frames += part.frames;
        tally.clipped += part.clipped;
        tally.non_finite += part.non_finite;
        tally.peak = tally.peak.max(part.peak);
        tally.output_tee_dropped += part.output_tee_dropped;
        tally.events += part.events;
    }
    tally
}

/// Count one callback's `tally` into `stats`, given the `used_us` it took,
/// and push a fault for each thing that went wrong.
fn account(
    stats: &Stats,
    faults: &mut rtrb::Producer<Fault>,
    tally: &Tally,
    used_us: u32,
    sample_rate: u32,
) {
    // accumulated in `fill`, then written once per callback rather than
    // once per sample
    stats
        .frames
        .fetch_add(tally.frames as u64, Ordering::Relaxed);
    stats.clipped.fetch_add(tally.clipped, Ordering::Relaxed);
    stats
        .non_finite
        .fetch_add(tally.non_finite, Ordering::Relaxed);
    stats.record_peak(tally.peak);
    stats
        .output_tee_dropped
        .fetch_add(tally.output_tee_dropped, Ordering::Relaxed);
    if tally.output_tee_dropped > 0 {
        let _ = faults.push(Fault::TeeDropped {
            frames: tally.output_tee_dropped,
        });
    }

    stats.callback_us_max.fetch_max(used_us, Ordering::Relaxed);
    stats.buffer_frames.store(tally.frames, Ordering::Relaxed);

    // the deadline comes from this callback's own frame count. the driver
    // may vary the buffer, so a ratio of two independent maxima would
    // describe no callback that ever ran.
    let budget_us = tally.frames as f32 / sample_rate as f32 * 1e6;
    if budget_us > 0.0 {
        stats.budget_us.store(budget_us as u32, Ordering::Relaxed);
        stats.record_load(used_us as f32 / budget_us);
        if used_us as f32 > budget_us {
            stats.overruns.fetch_add(1, Ordering::Relaxed);
            let _ = faults.push(Fault::Overrun {
                used_us,
                budget_us: budget_us as u32,
            });
        }
    }
}

/// Count a stream error and push its kind; the driver's message stays
/// behind, since reading it means formatting.
fn stream_error(stats: &Stats, faults: &mut rtrb::Producer<Fault>, error: &cpal::Error) {
    stats.errors.fetch_add(1, Ordering::Relaxed);
    let _ = faults.push(Fault::Stream(stream_kind(error.kind())));
}

fn stream_kind(kind: cpal::ErrorKind) -> StreamKind {
    use cpal::ErrorKind as Cpal;
    match kind {
        Cpal::Xrun => StreamKind::Underrun,
        Cpal::DeviceNotAvailable => StreamKind::DeviceGone,
        Cpal::DeviceChanged => StreamKind::DeviceChanged,
        Cpal::DeviceBusy => StreamKind::DeviceBusy,
        Cpal::StreamInvalidated => StreamKind::Invalidated,
        Cpal::RealtimeDenied => StreamKind::RealtimeDenied,
        Cpal::BackendError => StreamKind::Backend,
        _ => StreamKind::Other,
    }
}

fn build<T>(
    output: &Output,
    mut player: Player,
    stats: Arc<Stats>,
    mut recorder: Option<Recorder>,
    (mut faults, mut error_faults): (rtrb::Producer<Fault>, rtrb::Producer<Fault>),
    render: fn(&mut [T], usize, &mut Player, &mut Applied) -> Tally,
) -> Result<cpal::Stream, Error>
where
    T: SizedSample + 'static,
{
    let channels = output.channels;
    let sample_rate = output.sample_rate;
    let error_stats = Arc::clone(&stats);
    let config = cpal::StreamConfig {
        buffer_size: output.buffer,
        ..output.config.config()
    };
    let stream = output
        .device
        .build_output_stream(
            config,
            move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
                let start = Instant::now();
                let at = info.timestamp();
                let output = at.playback.duration_since(at.callback);
                let mut timing_dropped = 0;
                let tally = render(out, channels, &mut player, &mut |events| {
                    if let Some(recorder) = &mut recorder {
                        timing_dropped += recorder.applied(events, start, output);
                    }
                });
                if let Some(recorder) = &mut recorder {
                    timing_dropped += recorder.output(output);
                }
                stats
                    .output_us
                    .store(output.as_micros() as u32, Ordering::Relaxed);
                if timing_dropped > 0 {
                    stats
                        .timing_dropped
                        .fetch_add(timing_dropped, Ordering::Relaxed);
                }

                let used_us = start.elapsed().as_micros() as u32;
                account(&stats, &mut faults, &tally, used_us, sample_rate);
            },
            // counted and kept as a kind, not printed: the tui owns the
            // screen, and this may run on the audio thread
            move |error| stream_error(&error_stats, &mut error_faults, &error),
            None,
        )
        .map_err(|source| Error::Stream {
            buffer: output.buffer,
            source,
        })?;
    stream.play().map_err(|source| Error::Play { source })?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::super::render::Feed;
    use super::*;
    use crate::spec::{Event, Freqs, Instrument, Segment, Timeline};

    fn player() -> Player {
        let flat = || Segment {
            curve: vec![1.0],
            ms: 0.0,
        };
        let instrument = Instrument::new(
            vec![0.25, -0.5, 1.0, -1.0].into(),
            flat(),
            flat(),
            1.0,
            flat(),
            1,
        )
        .unwrap();
        let mut hz = [0.0; 128];
        hz[1] = 1.0;
        let on = Event::NoteOn {
            instrument: 0,
            note: 1,
            velocity: 127,
        };
        Player::new(
            Feed::Timeline(Timeline {
                instruments: vec![instrument],
                events: vec![(0, on)],
                total_samples: 4096,
            }),
            Freqs::unchecked(hz),
            4.0,
        )
    }

    // three channels do not divide SCRATCH, so a pass that split a frame
    // would show here as a shifted or repeated sample
    #[test]
    fn convert_matches_fill_across_passes() {
        let channels = 3;
        let mut expected = vec![0.0f32; channels * 1000];
        let want = fill(&mut expected, channels, &mut player());

        let mut out = vec![0i16; expected.len()];
        let mut passes = Vec::new();
        let got = convert(&mut out, channels, &mut player(), &mut |events| {
            passes.push(events);
        });

        assert_eq!(got, want);
        let converted: Vec<i16> = expected.iter().map(|&s| i16::from_sample(s)).collect();
        assert_eq!(out, converted);
        assert_eq!(passes, vec![1, 0, 0]);
    }

    // a count, not a gauge: taking a snapshot must not reset it
    #[test]
    fn dropped_events_accumulate_across_snapshots() {
        let stats = Stats::default();
        stats.record_dropped();
        stats.record_dropped();
        assert_eq!(stats.snapshot().dropped, 2);
        assert_eq!(stats.snapshot().dropped, 2);
    }

    fn overrun() -> Tally {
        Tally {
            frames: 48,
            output_tee_dropped: 2,
            ..Tally::default()
        }
    }

    #[test]
    fn an_overrun_and_a_full_output_tee_are_faults() {
        let stats = Stats::default();
        let mut faults = Faults::new();
        let mut tx = faults.ring();
        // 48 frames at 48 kHz is a 1000 µs deadline
        account(&stats, &mut tx, &overrun(), 1500, 48_000);
        faults.drain();
        assert_eq!(
            faults.last(),
            Some(Fault::Overrun {
                used_us: 1500,
                budget_us: 1000
            })
        );
        assert_eq!(stats.snapshot().overruns, 1);
    }

    #[test]
    fn a_stream_error_keeps_its_kind() {
        let stats = Stats::default();
        let mut faults = Faults::new();
        let mut tx = faults.ring();
        stream_error(&stats, &mut tx, &cpal::Error::new(cpal::ErrorKind::Xrun));
        faults.drain();
        assert_eq!(faults.last(), Some(Fault::Stream(StreamKind::Underrun)));
        assert_eq!(stats.snapshot().errors, 1);
    }

    // a full ring loses the record, and the count still moves
    #[test]
    fn faults_never_allocate_whether_the_ring_has_room_or_is_full() {
        let stats = Stats::default();
        let mut faults = Faults::new();
        let mut tx = faults.ring();
        let tally = overrun();
        let error = cpal::Error::new(cpal::ErrorKind::Xrun);
        let room = tx.slots();
        let info = allocation_counter::measure(|| {
            for _ in 0..room {
                account(&stats, &mut tx, &tally, 1500, 48_000);
                stream_error(&stats, &mut tx, &error);
            }
        });
        assert_eq!(info.count_total, 0);
        assert_eq!(tx.slots(), 0);
        assert_eq!(stats.snapshot().overruns, room as u64);
        assert_eq!(stats.snapshot().errors, room as u64);
    }

    const RANGE: cpal::SupportedBufferSize =
        cpal::SupportedBufferSize::Range { min: 64, max: 4096 };

    #[test]
    fn no_setting_leaves_the_buffer_to_the_platform() {
        assert_eq!(
            buffer_size(None, &RANGE).unwrap(),
            cpal::BufferSize::Default
        );
    }

    #[test]
    fn a_size_in_range_is_asked_for() {
        assert_eq!(
            buffer_size(Some(128), &RANGE).unwrap(),
            cpal::BufferSize::Fixed(128)
        );
    }

    #[test]
    fn a_size_outside_the_device_range_is_named() {
        let error = buffer_size(Some(32), &RANGE).unwrap_err();
        assert!(
            matches!(
                error,
                Error::BufferFrames {
                    frames: 32,
                    min: 64,
                    max: 4096
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn a_device_with_no_range_gets_the_request() {
        let unknown = cpal::SupportedBufferSize::Unknown;
        assert_eq!(
            buffer_size(Some(32), &unknown).unwrap(),
            cpal::BufferSize::Fixed(32)
        );
    }
}
