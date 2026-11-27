//! A live session, in the Elm shape `RULES.md` names: a `Model`, the
//! `Msg`s that change it, and the `Cmd`s they ask for, which the
//! session carries out on the output device and the ring into the audio
//! callback.
//!
//! A view reads the model and sends messages, but never builds a session,
//! so a second view can reuse this wiring unchanged.

mod latency;

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::audio::{self, Feed, Origin, Player, Record, Stamp, Stats};
use crate::input::keyboard::{Gating, Keyboard, Outcome, Stroke};
use crate::script;
use crate::spec::{Config, Event};

pub(crate) use latency::{Latency, Spread, Window};

/// What a view sends the session. A view reads its own library's input and
/// translates it into one of these, so the session never names that library.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Msg {
    /// A key, and when it was read.
    Key(Stroke, Instant),
    /// Once per poll: ends every `Fixed` note whose time is up.
    Tick,
}

/// What `update` asks the session to carry out, so `update` itself touches
/// no ring or device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Cmd {
    /// an event, and what caused it, carried to the push so it can be timed
    Play(Event, Origin),
}

/// Everything a view reads. A view gets only `&Model`, so `update` is the
/// one way it changes.
pub(crate) struct Model {
    pub(crate) keyboard: Keyboard,
    /// The same baked table the player reads, copied for the view.
    pub(crate) freqs: crate::spec::Freqs,
    /// Whether the help overlay is up.
    pub(crate) help: bool,
}

impl Model {
    fn update(&mut self, msg: Msg, cmds: &mut Vec<Cmd>) -> ControlFlow<()> {
        let mut events = Vec::new();
        let (flow, origin) = match msg {
            Msg::Key(stroke, read) => {
                // the overlay owns the keyboard while it is up: no key
                // reaches the note grid, so nothing can be left sounding
                // behind it. quit still works, and the help key toggles.
                let outcome = if self.help {
                    self.keyboard.global(stroke)
                } else {
                    self.keyboard.handle(stroke, &mut events)
                };
                let flow = match outcome {
                    Outcome::Quit => ControlFlow::Break(()),
                    Outcome::Help => {
                        self.help = !self.help;
                        ControlFlow::Continue(())
                    }
                    Outcome::Continue => ControlFlow::Continue(()),
                };
                (flow, Origin::Key { read })
            }
            Msg::Tick => {
                // a no-op when Held
                self.keyboard.tick(&mut events);
                (ControlFlow::Continue(()), Origin::Release)
            }
        };
        cmds.extend(events.into_iter().map(|event| Cmd::Play(event, origin)));
        flow
    }

    pub(crate) fn glance(&self) -> Glance {
        Glance {
            sounding: self.keyboard.sounding(),
            last_key: self.keyboard.last_key(),
            octave: self.keyboard.octave(),
            reach: self.keyboard.reach(),
            help: self.help,
        }
    }
}

/// The part of the model that changes while playing, copied out so a view on
/// another thread can draw it without sharing the keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Glance {
    pub(crate) sounding: Option<u8>,
    pub(crate) last_key: Option<char>,
    /// The current shift, and whether it is the one the keyboard started at.
    pub(crate) octave: (i32, bool),
    /// The lowest and highest shift the octave keys can reach.
    pub(crate) reach: (i32, i32),
    pub(crate) help: bool,
}

/// The model and the ring producers: everything a message touches.
///
/// `Send` and apart from the stream, so whichever thread reads input owns it
/// and pushes each event the moment it is read.
pub(crate) struct Input {
    model: Model,
    /// The ring's producer; the consumer lives in the audio callback.
    tx: rtrb::Producer<Event>,
    /// One stamp per event in `tx`, pushed just before it.
    stamps: rtrb::Producer<Stamp>,
    stats: Arc<Stats>,
}

impl Input {
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
            // stalled, and dropping an event is better than blocking input
            Cmd::Play(event, origin) => {
                // checked together, so a stamp is never left without its
                // event; nothing else pushes, so room cannot shrink after
                if self.stamps.slots() == 0 || self.tx.slots() == 0 {
                    self.stats.record_dropped();
                    return;
                }
                let pushed = Instant::now();
                // the stamp goes first, so the callback never pops an
                // event whose stamp is not there yet
                if self.stamps.push(Stamp { pushed, origin }).is_err()
                    || self.tx.push(event).is_err()
                {
                    self.stats.record_dropped();
                }
            }
        }
    }
}

/// The view's side of timing: what the callback records, folded into
/// windows, and the view's own draw rate.
pub(crate) struct Timing {
    pub(crate) latency: Latency,
    /// Frames per callback asked of the device; `None` left it to the
    /// platform.
    pub(crate) requested: Option<u32>,
    records: rtrb::Consumer<Record>,
}

impl Timing {
    /// The view finished drawing a frame at `at`.
    pub(crate) fn drawn(&mut self, at: Instant) {
        self.latency.view.push(at);
    }

    /// Fold in every record the callback has pushed, as of `now`.
    pub(crate) fn drain(&mut self, now: Instant) {
        while let Ok(record) = self.records.pop() {
            self.latency.record(now, record);
        }
        self.latency.prune(now);
    }
}

/// A patch playing live on the default output device.
pub struct Session {
    input: Input,
    timing: Timing,
    pub(crate) stats: Arc<Stats>,
    /// Kept only so the stream plays until the session drops.
    _stream: audio::Stream,
}

impl Session {
    /// Open the device, load the patch at `path` (or the default without
    /// one), and start it playing from the keyboard.
    pub fn open(path: Option<&Path>, gating: Gating, config: &Config) -> anyhow::Result<Self> {
        let output = audio::open(config.buffer_frames)?;
        let patch = match path {
            Some(path) => script::load(path, output.sample_rate, config.custom_dir.as_deref())?,
            None => script::load_default(output.sample_rate, config.custom_dir.as_deref())?,
        };

        // the view reads the same baked table the player does,
        // and the player takes ownership of it.
        // 128 f32 copied once at startup.
        let freqs = patch.freqs.clone();

        // the producer stays here; the consumer moves into the audio callback
        let (tx, rx) = rtrb::RingBuffer::<Event>::new(config.queue);
        let player = Player::new(
            Feed::Live {
                instruments: patch.timeline.instruments,
                events: rx,
            },
            patch.freqs,
            output.sample_rate as f32,
        )
        .with_filter(patch.filter);

        let (stamps, recorder, records) = audio::timing(config.queue, config.tui.latency_records);
        let stats = Arc::new(Stats::default());
        let stream = audio::stream(&output, player, Arc::clone(&stats), Some(recorder))?;

        Ok(Self {
            input: Input {
                model: Model {
                    keyboard: Keyboard::new(patch.keymap, gating),
                    freqs,
                    help: false,
                },
                tx,
                stamps,
                stats: Arc::clone(&stats),
            },
            timing: Timing {
                latency: Latency::new(config.tui.latency_window),
                requested: output.requested(),
                records,
            },
            stats,
            _stream: stream,
        })
    }

    pub(crate) fn model(&self) -> &Model {
        self.input.model()
    }

    /// The input side, lent to the thread that reads keys, beside the timing
    /// and stats the view keeps.
    pub(crate) fn split(&mut self) -> (&mut Input, &mut Timing, &Stats) {
        (&mut self.input, &mut self.timing, &self.stats)
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
            freqs: crate::spec::Freqs::unchecked([0.0; 128]),
            help: false,
        }
    }

    fn key(ch: char, ctrl: bool) -> Msg {
        Msg::Key(
            Stroke {
                key: Key::Char(ch),
                kind: Kind::Press,
                ctrl,
            },
            read(),
        )
    }

    // one instant for every key, so a test can name the trigger it expects
    fn read() -> Instant {
        static READ: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *READ.get_or_init(Instant::now)
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
            vec![Cmd::Play(
                Event::NoteOn {
                    instrument: 0,
                    note: 60,
                    velocity: 100,
                },
                Origin::Key { read: read() },
            )]
        );
    }

    #[test]
    fn a_drawn_frame_is_counted() {
        let (_, _, records) = audio::timing(1, 1);
        let mut timing = Timing {
            latency: Latency::new(Duration::from_secs(5)),
            requested: None,
            records,
        };
        timing.drawn(Instant::now());
        assert_eq!(timing.latency.view.per_second(), 1);
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

    #[test]
    fn input_can_be_owned_by_another_thread() {
        fn send<T: Send>() {}
        send::<Input>();
    }

    #[test]
    fn the_glance_shows_what_a_key_changed() {
        let mut model = model();
        let before = model.glance();
        send(&mut model, key('a', false));
        let after = model.glance();
        assert_eq!(after.sounding, Some(60));
        assert_eq!(after.last_key, Some('a'));
        assert_ne!(before, after);
        send(&mut model, key('?', false));
        assert!(model.glance().help);
    }
}
