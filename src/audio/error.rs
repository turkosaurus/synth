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
