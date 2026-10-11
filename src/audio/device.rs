use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use super::render::{fill, Player};

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
        self.peak_bits.fetch_max(peak.abs().to_bits(), Ordering::Relaxed);
    }

    fn record_load(&self, load: f32) {
        self.load_bits.fetch_max(load.to_bits(), Ordering::Relaxed);
    }

    fn peak(&self) -> f32 {
        f32::from_bits(self.peak_bits.load(Ordering::Relaxed))
    }

    /// How long one callback has before the buffer it was handed is late.
    /// Polyphony needs this as a number to know what it can afford, and
    /// asking must not disturb the gauges, so this does not reset.
    pub fn budget_us(&self) -> u32 {
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
            peak: self.peak(),
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
    pub device: cpal::Device,
    pub config: cpal::SupportedStreamConfig,
    pub sample_rate: u32,
    pub channels: usize,
}

/// Open the default output device. Separated from playback so every view and
/// command negotiates the device the same way.
pub fn open() -> anyhow::Result<Output> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no default output device")?;
    let config = device.default_output_config()?;
    anyhow::ensure!(
        config.sample_format() == cpal::SampleFormat::F32,
        "device wants {:?}, only f32 is wired up",
        config.sample_format()
    );

    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    debug!(sample_rate, channels, format = ?config.sample_format(), "device");

    Ok(Output {
        device,
        config,
        sample_rate,
        channels,
    })
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
        output.config.clone().into(),
        move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let start = Instant::now();
            let tally = fill(out, channels, &mut player);

            // accumulated in `fill`, then written once per callback rather
            // than once per sample
            stats.frames.fetch_add(tally.frames as u64, Ordering::Relaxed);
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
