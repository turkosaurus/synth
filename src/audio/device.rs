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
    callback_us_max: AtomicU32,
    frames_max: AtomicU32,
    samples: AtomicU64,
    clips: AtomicU64,
    peak_bits: AtomicU32,
}

impl Stats {
    // for non-negative floats the IEEE-754 bit pattern orders the same as
    // the value, so fetch_max on the bits is a correct max on the float
    fn record_peak(&self, peak: f32) {
        self.peak_bits.fetch_max(peak.abs().to_bits(), Ordering::Relaxed);
    }

    pub fn peak(&self) -> f32 {
        f32::from_bits(self.peak_bits.load(Ordering::Relaxed))
    }

    pub fn report(&self, sample_rate: u32) -> String {
        let frames = self.frames_max.load(Ordering::Relaxed);
        let budget_us = frames as f64 / sample_rate as f64 * 1e6;
        let used_us = self.callback_us_max.load(Ordering::Relaxed);
        format!(
            "samples={} clips={} peak={:.3} callback_max={}us budget={:.0}us ({:.1}%)",
            self.samples.load(Ordering::Relaxed),
            self.clips.load(Ordering::Relaxed),
            self.peak(),
            used_us,
            budget_us,
            used_us as f64 / budget_us * 100.0,
        )
    }
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
    let stream = output.device.build_output_stream(
        output.config.clone().into(),
        move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let start = Instant::now();
            let tally = fill(out, channels, &mut player);

            // accumulated in `fill`, then written once per callback rather
            // than once per sample
            stats.samples.fetch_add(tally.frames as u64, Ordering::Relaxed);
            stats.clips.fetch_add(tally.clips, Ordering::Relaxed);
            stats.frames_max.fetch_max(tally.frames, Ordering::Relaxed);
            stats.record_peak(tally.peak);
            stats
                .callback_us_max
                .fetch_max(start.elapsed().as_micros() as u32, Ordering::Relaxed);
        },
        |err| eprintln!("stream error: {err}"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}
