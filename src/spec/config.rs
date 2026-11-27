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
    pub tui: TuiConfig,
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
    /// The latency scale's left end, in ms; below `latency_ceil_ms`.
    pub latency_floor_ms: f32,
    /// The latency scale's right end, in ms.
    pub latency_ceil_ms: f32,
    pub input_budget: Budget,
    pub render_budget: Budget,
    pub output_budget: Budget,
    pub total_budget: Budget,
    /// Frames per second below which the view's rate turns yellow.
    pub fps_warn: u32,
    /// Frames per second below which it turns red; at most `fps_warn`.
    pub fps_danger: u32,
}

/// Where a latency stage turns yellow, then red, in ms; `warn_ms` is at
/// most `danger_ms`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Budget {
    pub warn_ms: f32,
    pub danger_ms: f32,
}
