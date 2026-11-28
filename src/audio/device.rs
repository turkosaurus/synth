use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use super::fault::{Fault, Faults, Reporter, StreamKind};
use super::render::{fill, push, Player, Tally};
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
    /// input frames the input callback could not push to the input tee
    /// because its ring was full
    input_tee_dropped: AtomicU64,
    /// reads of input that failed, each of which ends the session
    input_errors: AtomicU64,
    peak_bits: AtomicU32,
    /// the input meter's channel, written by the input callback
    input_peak_bits: AtomicU32,
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

    fn record_input_peak(&self, peak: f32) {
        self.input_peak_bits
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

    pub(crate) fn record_input_error(&self) {
        self.input_errors.fetch_add(1, Ordering::Relaxed);
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
            input_errors: self.input_errors.load(Ordering::Relaxed),
            input_tee_dropped: self.input_tee_dropped.load(Ordering::Relaxed),
            peak: self.take_peak(),
            input_peak: f32::from_bits(self.input_peak_bits.swap(0, Ordering::Relaxed)),
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
    pub input_errors: u64,
    pub input_tee_dropped: u64,
    pub peak: f32,
    /// the loudest sample of the input meter's channel, 0.0 with no input
    pub input_peak: f32,
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
    /// the same device's input, when it has one synth can read
    pub capture: Option<Capture>,
}

/// The input side of the output's device. Only the same device: it shares
/// the output's clock, so the two cannot drift apart.
pub struct Capture {
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
    if !readable(format) {
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
    let capture = capture_config(&device, buffer_frames);

    Ok(Output {
        device,
        config,
        buffer,
        sample_rate,
        channels,
        capture,
    })
}

/// The device's default input, if it has one in a format synth reads. Its
/// buffer is the output's when the input takes that size, and the
/// platform's when not, since a scope needs no particular one.
fn capture_config(device: &cpal::Device, buffer_frames: Option<u32>) -> Option<Capture> {
    let config = match device.default_input_config() {
        Ok(config) => config,
        Err(error) => {
            debug!(%error, "no input");
            return None;
        }
    };
    let format = config.sample_format();
    if !readable(format) || config.channels() == 0 {
        debug!(?format, channels = config.channels(), "input unread");
        return None;
    }
    let buffer = buffer_size(buffer_frames, config.buffer_size()).unwrap_or_default();
    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    debug!(sample_rate, channels, ?format, ?buffer, "input");
    Some(Capture {
        config,
        buffer,
        sample_rate,
        channels,
    })
}

fn readable(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::I32
    )
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
fn account(stats: &Stats, faults: &mut Reporter, tally: &Tally, used_us: u32, sample_rate: u32) {
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
        faults.push(Fault::TeeDropped {
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
            faults.push(Fault::Overrun {
                used_us,
                budget_us: budget_us as u32,
            });
        }
    }
}

/// Count a stream error and push its kind, as `fault` wraps it; the
/// driver's message stays behind, since reading it means formatting.
fn stream_error(
    stats: &Stats,
    faults: &mut Reporter,
    error: &cpal::Error,
    fault: fn(StreamKind) -> Fault,
) {
    stats.errors.fetch_add(1, Ordering::Relaxed);
    faults.push(fault(stream_kind(error.kind())));
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
    (mut faults, mut error_faults): (Reporter, Reporter),
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
            move |error| stream_error(&error_stats, &mut error_faults, &error, Fault::Stream),
            None,
        )
        .map_err(|source| Error::Stream {
            buffer: output.buffer,
            source,
        })?;
    stream.play().map_err(|source| Error::Play { source })?;
    Ok(stream)
}

/// Build and start the input stream on `output`'s device, pushing every
/// frame to `tee` and the peak of `channel` to `stats`. `None` when the
/// device has no input, or when its stream cannot start, which is counted
/// and pushed as a fault: the output plays on either way.
pub fn capture(
    output: &Output,
    tee: rtrb::Producer<f32>,
    channel: Channel,
    stats: Arc<Stats>,
    faults: &mut Faults,
) -> Option<cpal::Stream> {
    let capture = output.capture.as_ref()?;
    // one ring each, as for the output
    let rings = (faults.ring(), faults.ring());
    let device = &output.device;
    let shared = Arc::clone(&stats);
    let built = match capture.config.sample_format() {
        SampleFormat::I16 => build_input::<i16>(device, capture, tee, channel, shared, rings),
        SampleFormat::I32 => build_input::<i32>(device, capture, tee, channel, shared, rings),
        _ => build_input::<f32>(device, capture, tee, channel, shared, rings),
    };
    match built {
        Ok(stream) => Some(stream),
        Err(error) => {
            debug!(%error, "input stream");
            stream_error(&stats, &mut faults.ring(), &error, Fault::InputStream);
            None
        }
    }
}

/// Which samples of each input frame the input meter reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// the loudest of every channel
    All,
    /// one channel, from 0
    One(usize),
}

/// What one input callback took.
#[derive(Debug, Default, PartialEq)]
struct Taken {
    /// largest finite magnitude on the meter's channel
    peak: f32,
    /// frames the input tee had no room for
    dropped: u64,
}

/// Push `data`'s whole frames to `tee` as f32, and find the peak of
/// `channel`. Split out of the callback so it can be tested without a
/// device.
fn take<T>(data: &[T], channels: usize, channel: Channel, tee: &mut rtrb::Producer<f32>) -> Taken
where
    T: Sample,
    f32: FromSample<T>,
{
    let samples = data.iter().map(|&sample| f32::from_sample(sample));
    let peak = samples
        .clone()
        .enumerate()
        .filter(|(i, _)| match channel {
            Channel::All => true,
            Channel::One(one) => i % channels == one,
        })
        .map(|(_, sample)| sample.abs())
        .filter(|sample| sample.is_finite())
        .fold(0.0, f32::max);
    Taken {
        peak,
        dropped: push(tee, samples, channels),
    }
}

/// Count one input callback's `taken` into `stats`, and push a fault when
/// its tee was full.
fn account_input(stats: &Stats, faults: &mut Reporter, taken: &Taken) {
    stats.record_input_peak(taken.peak);
    if taken.dropped > 0 {
        stats
            .input_tee_dropped
            .fetch_add(taken.dropped, Ordering::Relaxed);
        faults.push(Fault::InputTeeDropped {
            frames: taken.dropped,
        });
    }
}

fn build_input<T>(
    device: &cpal::Device,
    capture: &Capture,
    mut tee: rtrb::Producer<f32>,
    channel: Channel,
    stats: Arc<Stats>,
    (mut faults, mut error_faults): (Reporter, Reporter),
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + 'static,
    f32: FromSample<T>,
{
    let channels = capture.channels;
    let error_stats = Arc::clone(&stats);
    let config = cpal::StreamConfig {
        buffer_size: capture.buffer,
        ..capture.config.config()
    };
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let taken = take(data, channels, channel, &mut tee);
            account_input(&stats, &mut faults, &taken);
        },
        // as the output's: counted and kept as a kind, never printed
        move |error| stream_error(&error_stats, &mut error_faults, &error, Fault::InputStream),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

#[cfg(test)]
mod tests;
