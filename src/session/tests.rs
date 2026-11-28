use std::collections::HashMap;
use std::time::Duration;

use super::*;
use crate::input::keyboard::Kind;
use crate::spec::{Key, Keymap};

pub(crate) fn model() -> Model {
    let keymap = Keymap {
        notes: HashMap::from([('a', 60)]),
        name: None,
        octave_up: vec![],
        octave_down: vec![],
        stop: vec![],
        scope: vec![Key::Tab],
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
        scope: Selection {
            stream: 0,
            pick: Pick::Mix,
        },
        streams: vec![2],
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
    let _ = model.update(msg, &mut Vec::new(), &mut cmds);
    cmds
}

fn quits(model: &mut Model, msg: Msg) -> bool {
    model
        .update(msg, &mut Vec::new(), &mut Vec::new())
        .is_break()
}

#[test]
fn the_scope_key_moves_the_scope() {
    let mut model = model();
    let tab = Msg::Key(
        Stroke {
            key: Key::Tab,
            kind: Kind::Press,
            ctrl: false,
        },
        read(),
    );
    assert!(send(&mut model, tab).is_empty());
    assert_eq!(model.scope.pick, Pick::Channel(0));
    assert_eq!(model.glance().scope, model.scope);
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
fn input_never_allocates_and_reports_a_full_queue() {
    let mut fixed = model();
    // a zero gate is due at the next tick
    fixed.keyboard = Keyboard::new(fixed.keyboard.keymap().clone(), Gating::Fixed);
    for model in [model(), fixed] {
        never_allocates(model);
    }
}

fn never_allocates(model: Model) {
    let (tx, _events) = rtrb::RingBuffer::new(1);
    let (stamps, _, _) = audio::timing(1, 1);
    let mut faults = Faults::new(1);
    let stats = Arc::new(Stats::default());
    let mut input = Input::new(model, tx, stamps, Arc::clone(&stats), faults.ring());
    let release = Msg::Key(
        Stroke {
            key: Key::Char('a'),
            kind: Kind::Release,
            ctrl: false,
        },
        read(),
    );
    let info = allocation_counter::measure(|| {
        // the note-on fills the queue, so its note-off is dropped,
        // from the release when Held and the tick when Fixed
        let _ = input.update(key('a', false));
        let _ = input.update(release);
        let _ = input.update(Msg::Tick);
        let _ = input.update(key('?', false));
    });
    assert_eq!(info.count_total, 0);
    assert_eq!(stats.snapshot().dropped, 1);
    faults.drain();
    assert_eq!(faults.last(), Some(Fault::Dropped));
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
