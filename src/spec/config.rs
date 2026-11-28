use std::path::PathBuf;
use std::time::Duration;

/// Settings for the machine synth runs on, from `config.lua`, as opposed to
/// a patch, which a song carries from one machine to another. Every field is
/// set: the embedded `lua/config.lua` supplies whatever a user's file leaves
/// out.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Config {
    /// Frames per device callback. `None` leaves it to the platform.
    pub buffer_frames: Option<u32>,
    /// Events the input side can queue before the audio thread reads them.
    pub queue: usize,
    /// Where `require` looks after the patch's own folder. `None` when there
    /// is no home directory to put the default in.
    pub custom_dir: Option<PathBuf>,
    pub capture: CaptureConfig,
    pub tui: TuiConfig,
    pub log: LogConfig,
}

/// What synth reads of the output device's capture.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct CaptureConfig {
    /// The channel the capture meter reads, from 1; 0 is the loudest of all.
    pub channel: usize,
}

/// How `log.jsonl` is kept between sessions.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct LogConfig {
    /// At start, a log larger than this many KiB keeps only its newest
    /// lines that fit; 0 never trims.
    pub trim_size_kb: u64,
}

/// How the terminal view runs and draws.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TuiConfig {
    /// Time between redraws, and the longest a key read waits.
    pub poll: Duration,
    /// How long a peak marker holds before it falls.
    pub meter_hold: Duration,
    /// How long a fall across the whole meter takes.
    pub meter_fall: Duration,
    /// The quietest level the meter shows, in dB below full scale.
    pub meter_floor_db: f32,
    /// How far back each latency row's readings reach.
    pub latency_window: Duration,
    /// Timing records the callback can push before the view drains them;
    /// the ring is allocated once, when the session opens.
    pub latency_records: usize,
    /// The latency scale's left end, in ms; below `latency_ceil_ms`.
    pub latency_floor_ms: f32,
    /// The latency scale's right end, in ms.
    pub latency_ceil_ms: f32,
    pub input_budget: Budget,
    pub render_budget: Budget,
    pub output_budget: Budget,
    pub total_budget: Budget,
    /// Frames per second below which the view's rate turns yellow, then
    /// red; `danger` is below `warn`, since a higher rate is better.
    pub fps: Budget,
    /// Faults each producing thread can push before the view drains them;
    /// each ring is allocated once, when the stream opens.
    pub faults: usize,
    /// The stream the scope reads at start.
    pub source_stream: StreamName,
    /// The channel the scope reads at start, from 1; 0 is the mix of all.
    pub source_channel: usize,
    /// How much of each stream its tee holds for the view to drain.
    pub source_tee: Duration,
    /// How much of its source the scope keeps to find a trigger in.
    pub scope_history: Duration,
    /// Cycles of the sounding pitch the scope's window spans.
    pub scope_cycles: f32,
    /// Samples per spectrum transform, a power of two.
    pub spectrum_size: usize,
    /// The quietest level the spectrum shows, in dB below full scale.
    pub spectrum_floor_db: f32,
    /// Percent of the scope pane the spectrum takes.
    pub spectrum_height: f32,
}

/// A stream of frames a view can read, by the name `config.lua` gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamName {
    /// what the device plays, after the clamp
    Output,
    /// what the output device's capture hears
    Capture,
}

impl StreamName {
    pub const ALL: [Self; 2] = [Self::Output, Self::Capture];

    pub fn name(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Capture => "capture",
        }
    }
}

/// Where a reading turns yellow, then red: ms for a latency stage, where
/// `warn` is below `danger`, or frames per second for the view.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Budget {
    pub warn: f32,
    pub danger: f32,
}
