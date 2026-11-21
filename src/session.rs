//! A live session, in the Elm shape `RULES.md` names: a [`Model`], the
//! [`Msg`]s that change it, and the [`Cmd`]s they ask for, which the
//! session carries out on the output device and the ring into the audio
//! callback.
//!
//! A view reads the model and sends messages, but never builds a session,
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

/// Everything a view reads. A view gets only `&Model`, so `update` is the
/// one way it changes.
pub(crate) struct Model {
    pub(crate) keyboard: Keyboard,
    /// The same baked table the player reads, copied for the view.
    pub(crate) freqs: Vec<f32>,
    /// Whether the help overlay is up.
    pub(crate) help: bool,
}

impl Model {
    fn update(&mut self, msg: Msg, cmds: &mut Vec<Cmd>) -> ControlFlow<()> {
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
}

/// A patch playing live on the default output device.
pub struct Session {
    model: Model,
    pub(crate) stats: Arc<Stats>,
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
            model: Model {
                keyboard: Keyboard::new(patch.keymap, gating),
                freqs,
                help: false,
            },
            stats,
            tx,
            _stream: stream,
        })
    }

    pub(crate) fn model(&self) -> &Model {
        &self.model
    }

    /// Apply `msg`, then carry out what it asked for; `Break` once the
    /// player has quit.
    pub(crate) fn update(&mut self, msg: Msg) -> ControlFlow<()> {
        let mut cmds = Vec::new();
        let flow = self.model.update(msg, &mut cmds);
        for cmd in cmds {
            self.run(cmd);
        }
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::*;
    use crate::input::keyboard::Kind;
    use crate::spec::{Key, Keymap};

    fn model() -> Model {
        let keymap = Keymap {
            notes: HashMap::from([('a', 60)]),
            name: None,
            octave_up: vec![],
            octave_down: vec![],
            stop: vec![],
            help: vec![Key::Char('?')],
            gate: Duration::ZERO,
            autorepeat: Duration::ZERO,
            doubletap: Duration::ZERO,
            velocity: 100,
            octave: 0,
            octave_low: 0,
            octave_high: 0,
        };
        Model {
            keyboard: Keyboard::new(keymap, Gating::Held),
            freqs: vec![],
            help: false,
        }
    }

    fn key(ch: char, ctrl: bool) -> Msg {
        Msg::Key(Stroke {
            key: Key::Char(ch),
            kind: Kind::Press,
            ctrl,
        })
    }

    fn send(model: &mut Model, msg: Msg) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        let _ = model.update(msg, &mut cmds);
        cmds
    }

    fn quits(model: &mut Model, msg: Msg) -> bool {
        model.update(msg, &mut Vec::new()).is_break()
    }

    #[test]
    fn a_note_key_plays() {
        let mut model = model();
        assert_eq!(
            send(&mut model, key('a', false)),
            vec![Cmd::Play(Event::NoteOn {
                instrument: 0,
                note: 60,
                velocity: 100,
            })]
        );
    }

    #[test]
    fn the_help_overlay_keeps_note_keys_silent() {
        let mut model = model();
        send(&mut model, key('?', false));
        assert!(model.help);
        assert_eq!(send(&mut model, key('a', false)), vec![]);
        send(&mut model, key('?', false));
        assert!(!model.help);
        assert_eq!(send(&mut model, key('a', false)).len(), 1);
    }

    #[test]
    fn ctrl_c_quits_with_or_without_the_overlay() {
        let mut model = model();
        assert!(quits(&mut model, key('c', true)));
        send(&mut model, key('?', false));
        assert!(quits(&mut model, key('c', true)));
    }
}
