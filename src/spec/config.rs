/// Settings for the machine synth runs on, from `config.lua`, as opposed to
/// a patch, which a song carries from one machine to another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// Frames per device callback. `None` leaves it to the platform.
    pub buffer_frames: Option<u32>,
}
