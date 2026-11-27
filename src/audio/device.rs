use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use super::render::{fill, Player, Tally};

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
    /// errors the driver reported, counted rather than logged because the
    /// error callback may run on the audio thread
    errors: AtomicU64,
    /// events the UI thread could not push because the ring was full
    dropped: AtomicU64,
    /// output samples the callback could not push to the tap because its
    /// ring was full
    tap_dropped: AtomicU64,
    peak_bits: AtomicU32,
    /// the most recent callback's deadline, so it describes a callback that
    /// actually ran
    budget_us: AtomicU32,
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
            tap_dropped: self.tap_dropped.load(Ordering::Relaxed),
            peak: self.take_peak(),
            budget_us: self.budget_us(),
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
    pub tap_dropped: u64,
    pub peak: f32,
    pub budget_us: u32,
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

/// Build and start a stream that drives `player`, reporting into `stats`.
pub fn stream(output: &Output, player: Player, stats: Arc<Stats>) -> Result<cpal::Stream, Error> {
    match output.config.sample_format() {
        SampleFormat::I16 => build::<i16>(output, player, stats, convert),
        SampleFormat::I32 => build::<i32>(output, player, stats, convert),
        _ => build::<f32>(output, player, stats, fill),
    }
}

/// Samples rendered per pass when the device is not f32: enough to keep
/// the passes few, small enough to live on the callback's stack.
const SCRATCH: usize = 1024;

/// Renders into a stack buffer and converts each sample to `T`, in passes
/// of whole frames so `fill` never sees a frame split across two of them.
fn convert<T>(out: &mut [T], channels: usize, player: &mut Player) -> Tally
where
    T: Sample + FromSample<f32>,
{
    let mut scratch = [0.0f32; SCRATCH];
    let pass = SCRATCH / channels * channels;
    let mut tally = Tally::default();
    for chunk in out.chunks_mut(pass) {
        let rendered = &mut scratch[..chunk.len()];
        let part = fill(rendered, channels, player);
        for (sample, &value) in chunk.iter_mut().zip(rendered.iter()) {
            *sample = T::from_sample(value);
        }
        tally.frames += part.frames;
        tally.clipped += part.clipped;
        tally.non_finite += part.non_finite;
        tally.peak = tally.peak.max(part.peak);
        tally.tap_dropped += part.tap_dropped;
    }
    tally
}

fn build<T>(
    output: &Output,
    mut player: Player,
    stats: Arc<Stats>,
    render: fn(&mut [T], usize, &mut Player) -> Tally,
) -> Result<cpal::Stream, Error>
where
    T: SizedSample + 'static,
{
    let channels = output.channels;
    let sample_rate = output.sample_rate;
    let faults = Arc::clone(&stats);
    let config = cpal::StreamConfig {
        buffer_size: output.buffer,
        ..output.config.config()
    };
    let stream = output
        .device
        .build_output_stream(
            config,
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
                let start = Instant::now();
                let tally = render(out, channels, &mut player);

                // accumulated in `fill`, then written once per callback rather
                // than once per sample
                stats
                    .frames
                    .fetch_add(tally.frames as u64, Ordering::Relaxed);
                stats.clipped.fetch_add(tally.clipped, Ordering::Relaxed);
                stats
                    .non_finite
                    .fetch_add(tally.non_finite, Ordering::Relaxed);
                stats.record_peak(tally.peak);
                stats
                    .tap_dropped
                    .fetch_add(tally.tap_dropped, Ordering::Relaxed);

                let used_us = start.elapsed().as_micros() as u32;
                stats.callback_us_max.fetch_max(used_us, Ordering::Relaxed);

                // the deadline comes from this callback's own frame count. the
                // driver may vary the buffer, so a ratio of two independent
                // maxima would describe no callback that ever ran.
                let budget_us = tally.frames as f32 / sample_rate as f32 * 1e6;
                if budget_us > 0.0 {
                    stats.budget_us.store(budget_us as u32, Ordering::Relaxed);
                    stats.record_load(used_us as f32 / budget_us);
                    if used_us as f32 > budget_us {
                        stats.overruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
            },
            // counted, not printed: the tui owns the screen, and this callback
            // may run on the audio thread, where formatting is not allowed
            move |_| {
                faults.errors.fetch_add(1, Ordering::Relaxed);
            },
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
        let got = convert(&mut out, channels, &mut player());

        assert_eq!(got, want);
        let converted: Vec<i16> = expected.iter().map(|&s| i16::from_sample(s)).collect();
        assert_eq!(out, converted);
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
