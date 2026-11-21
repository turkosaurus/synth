//! The wiring behind a live session: the output device, the patch, and the
//! ring from the UI thread into the audio callback.
//!
//! A view drives a [`Session`] and draws it but never builds one,
//! so a second view can reuse this wiring unchanged.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use crate::audio::{self, Player, Stats};
use crate::input::keyboard::{Gating, Keyboard, Outcome, Stroke};
use crate::script;
use crate::spec::{Config, Event};

/// What a view sends the session. A view reads its own library's input and
/// translates it into one of these, so the session never names that library.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Msg {
    Key(Stroke),
    /// Once per frame: ends every `Fixed` note whose time is up.
    Tick,
}

/// What `update` asks the session to carry out, so `update` itself touches
/// no ring or device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Cmd {
    Play(Event),
}

/// A patch playing live on the default output device.
pub struct Session {
    pub(crate) keyboard: Keyboard,
    pub(crate) stats: Arc<Stats>,
    /// The same baked table the player reads, copied for the view.
    pub(crate) freqs: Vec<f32>,
    help: bool,
    /// The ring's producer; the consumer lives in the audio callback.
    tx: rtrb::Producer<Event>,
    /// Kept only so the stream plays until the session drops.
    _stream: audio::Stream,
}

impl Session {
    /// Open the device, load the patch at `path` (or the default without
    /// one), and start it playing from the keyboard.
    pub fn open(path: Option<&Path>, gating: Gating, config: &Config) -> anyhow::Result<Self> {
        let output = audio::open(config.buffer_frames)?;
        let patch = match path {
            Some(path) => script::load(path, output.sample_rate, config.patch_dir.as_deref())?,
            None => script::load_default(output.sample_rate, config.patch_dir.as_deref())?,
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
            keyboard: Keyboard::new(patch.keymap, gating),
            stats,
            freqs,
            help: false,
            tx,
            _stream: stream,
        })
    }

    /// Whether the help overlay is up.
    pub(crate) fn help(&self) -> bool {
        self.help
    }

    /// Apply `msg`, then carry out what it asked for; `Break` once the
    /// player has quit.
    pub(crate) fn update(&mut self, msg: Msg) -> ControlFlow<()> {
        let mut cmds = Vec::new();
        let flow = self.apply(msg, &mut cmds);
        for cmd in cmds {
            self.run(cmd);
        }
        flow
    }

    fn apply(&mut self, msg: Msg, cmds: &mut Vec<Cmd>) -> ControlFlow<()> {
        let mut events = Vec::new();
        let flow = match msg {
            Msg::Key(stroke) => {
                // the overlay owns the keyboard while it is up: no key
                // reaches the note grid, so nothing can be left sounding
                // behind it. quit still works, and the help key toggles.
                let outcome = if self.help {
                    self.keyboard.global(stroke)
                } else {
                    self.keyboard.handle(stroke, &mut events)
                };
                match outcome {
                    Outcome::Quit => ControlFlow::Break(()),
                    Outcome::Help => {
                        self.help = !self.help;
                        ControlFlow::Continue(())
                    }
                    Outcome::Continue => ControlFlow::Continue(()),
                }
            }
            Msg::Tick => {
                // a no-op when Held
                self.keyboard.tick(&mut events);
                ControlFlow::Continue(())
            }
        };
        cmds.extend(events.into_iter().map(Cmd::Play));
        flow
    }

    fn run(&mut self, cmd: Cmd) {
        match cmd {
            // the queue is bounded; a full queue means the audio thread has
            // stalled, and dropping an event is better than blocking the UI
            Cmd::Play(event) => {
                let _ = self.tx.push(event);
            }
        }
    }
}
