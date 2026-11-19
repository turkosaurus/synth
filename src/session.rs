//! The wiring behind a live session: the output device, the patch, and the
//! ring from the UI thread into the audio callback.
//!
//! A view drives a [`Session`] and draws it but never builds one,
//! so a second view can reuse this wiring unchanged.

use std::path::Path;
use std::sync::Arc;

use crate::audio::{self, Player, Stats};
use crate::input::keyboard::{Gating, Keyboard};
use crate::script;
use crate::spec::{Config, Event};

/// A patch playing live on the default output device.
pub struct Session {
    /// Holds the ring's producer; the consumer lives in the audio callback.
    pub(crate) keyboard: Keyboard,
    pub(crate) stats: Arc<Stats>,
    /// The same baked table the player reads, copied for the view.
    pub(crate) freqs: Vec<f32>,
    /// Kept only so the stream plays until the session drops.
    _stream: audio::Stream,
}

impl Session {
    /// Open the device, load the patch at `path` (or the default without
    /// one), and start it playing from the keyboard.
    pub fn open(path: Option<&Path>, gating: Gating, config: &Config) -> anyhow::Result<Self> {
        let output = audio::open(config.buffer_frames)?;
        let patch = match path {
            Some(path) => script::load(path, output.sample_rate, config.patches.as_deref())?,
            None => script::load_default(output.sample_rate, config.patches.as_deref())?,
        };

        // the view reads the same baked table the player does,
        // and the player takes ownership of it.
        // 128 f32 copied once at startup.
        let freqs = patch.freqs.clone();

        // the producer stays here; the consumer moves into the audio callback
        let (tx, rx) = rtrb::RingBuffer::<Event>::new(config.queue);
        let player = Player::live(
            patch.timeline.instruments,
            patch.freqs,
            rx,
            output.sample_rate as f32,
        )
        .with_filter(patch.filter);

        let stats = Arc::new(Stats::default());
        let stream = audio::stream(&output, player, Arc::clone(&stats))?;

        Ok(Self {
            keyboard: Keyboard::new(patch.keymap, gating, tx),
            stats,
            freqs,
            _stream: stream,
        })
    }
}
