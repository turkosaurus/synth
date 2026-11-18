use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use super::render::{fill, Player};

/// A running output stream; audio stops when it drops. Named here so
/// holders need not import cpal, which stays in this file.
pub use cpal::Stream;

// written by the audio thread, read anywhere, lock-free because the
// callback must never block, and Relaxed because these are standalone
// counters, not gates on other memory.
#[derive(Default)]
pub struct Stats {
    /// frames, not samples: one frame is one sample per channel
    frames: AtomicU64,
    clips: AtomicU64,
    /// callbacks that ran past their deadline, which is the count that
    /// corresponds to something audible
    overruns: AtomicU64,
    /// errors the driver reported, counted rather than logged because the
    /// error callback may run on the audio thread
    errors: AtomicU64,
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
            clips: self.clips.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
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
pub struct Snapshot {
    pub frames: u64,
    pub clips: u64,
    pub overruns: u64,
    pub errors: u64,
    pub peak: f32,
    pub budget_us: u32,
    pub callback_us_max: u32,
    /// worst fraction of the deadline used, 1.0 being the deadline itself
    pub load: f32,
}

/// A negotiated output device, ready to build a stream on.
pub struct Output {
    pub(crate) device: cpal::Device,
    pub(crate) config: cpal::SupportedStreamConfig,
    pub(crate) buffer: cpal::BufferSize,
    pub sample_rate: u32,
    pub channels: usize,
}

/// Open the default output device. Separated from playback so every view and
/// command negotiates the device the same way.
pub fn open() -> anyhow::Result<Output> {
    // PulseAudio when a sound server is running, else ALSA
    let host = cpal::default_host();
    debug!(host = host.id().name(), "audio host");
    let device = host
        .default_output_device()
        .context("no default output device")?;
    let default = device.default_output_config()?;
    // the PulseAudio host defaults to an integer format but offers f32 too,
    // so ask for f32 at the default rate and channel count before giving up
    let config = if default.sample_format() == cpal::SampleFormat::F32 {
        default
    } else {
        let rate = default.sample_rate();
        device
            .supported_output_configs()?
            .find(|range| {
                range.sample_format() == cpal::SampleFormat::F32
                    && range.channels() == default.channels()
                    && (range.min_sample_rate()..=range.max_sample_rate()).contains(&rate)
            })
            .map(|range| range.with_sample_rate(rate))
            .with_context(|| {
                format!(
                    "device wants {:?} and offers no f32 at {rate} Hz; only f32 is wired up",
                    default.sample_format()
                )
            })?
    };

    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    debug!(sample_rate, channels, format = ?config.sample_format(), "device");

    let buffer = buffer_size(host.id(), config.buffer_size());
    debug!(?buffer, "buffer");

    Ok(Output {
        device,
        config,
        buffer,
        sample_rate,
        channels,
    })
}

/// Frames per callback asked of the PulseAudio host: about 22 ms from callback
/// to speaker at 48 kHz, against about 2 s when the server picks.
const PULSE_FRAMES: u32 = 256;

/// The buffer to ask the host for. PulseAudio, given no request, queues about
/// 2 s of audio, which is fine for a sequence but makes live keys lag.
/// Every other host's default is already short, so it is left alone.
fn buffer_size(host: cpal::HostId, supported: &cpal::SupportedBufferSize) -> cpal::BufferSize {
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd"
    ))]
    if host == cpal::HostId::PulseAudio {
        let frames = match *supported {
            cpal::SupportedBufferSize::Range { min, max } => PULSE_FRAMES.clamp(min, max.max(min)),
            cpal::SupportedBufferSize::Unknown => PULSE_FRAMES,
        };
        return cpal::BufferSize::Fixed(frames);
    }
    let _ = (host, supported);
    cpal::BufferSize::Default
}

/// Build and start a stream that drives `player`, reporting into `stats`.
pub fn stream(
    output: &Output,
    mut player: Player,
    stats: Arc<Stats>,
) -> anyhow::Result<cpal::Stream> {
    let channels = output.channels;
    let sample_rate = output.sample_rate;
    let faults = Arc::clone(&stats);
    let stream = output.device.build_output_stream(
        cpal::StreamConfig {
            buffer_size: output.buffer,
            ..output.config.into()
        },
        move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let start = Instant::now();
            let tally = fill(out, channels, &mut player);

            // accumulated in `fill`, then written once per callback rather
            // than once per sample
            stats
                .frames
                .fetch_add(tally.frames as u64, Ordering::Relaxed);
            stats.clips.fetch_add(tally.clips, Ordering::Relaxed);
            stats.record_peak(tally.peak);

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
    )?;
    stream.play()?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn pulseaudio_gets_a_short_fixed_buffer() {
        let range = cpal::SupportedBufferSize::Range { min: 64, max: 8192 };
        assert_eq!(
            buffer_size(cpal::HostId::PulseAudio, &range),
            cpal::BufferSize::Fixed(PULSE_FRAMES)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pulseaudio_buffer_stays_in_the_supported_range() {
        let range = cpal::SupportedBufferSize::Range { min: 512, max: 8192 };
        assert_eq!(
            buffer_size(cpal::HostId::PulseAudio, &range),
            cpal::BufferSize::Fixed(512)
        );
        let unknown = cpal::SupportedBufferSize::Unknown;
        assert_eq!(
            buffer_size(cpal::HostId::PulseAudio, &unknown),
            cpal::BufferSize::Fixed(PULSE_FRAMES)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn alsa_keeps_its_default() {
        let range = cpal::SupportedBufferSize::Range { min: 64, max: 8192 };
        assert_eq!(
            buffer_size(cpal::HostId::Alsa, &range),
            cpal::BufferSize::Default
        );
    }
}
