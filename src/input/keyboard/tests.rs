use std::time::Duration;

use super::*;

const REPEAT: Duration = Duration::from_millis(750);
const DOUBLETAP: Duration = Duration::from_millis(400);

fn keyboard(gating: Gating, gate: Duration) -> (Keyboard, Vec<Event>) {
    let keymap = Keymap {
        notes: HashMap::from([('a', 60), ('s', 62), ('A', 72)]),
        name: None,
        octave_up: vec![],
        octave_down: vec![],
        stop: vec![Key::Backspace],
        scope: vec![Key::Tab],
        help: vec![],
        quit: vec![],
        quit_now: vec![Key::Ctrl('c')],
        gate,
        autorepeat: REPEAT,
        doubletap: DOUBLETAP,
        velocity: 100,
        octave: 0,
        octave_low: 4,
        octave_high: 4,
    };
    (Keyboard::new(keymap, gating), Vec::new())
}

fn stroke(key: Key, kind: Kind) -> Stroke {
    Stroke {
        key,
        kind,
        ctrl: false,
    }
}

fn press(keyboard: &mut Keyboard, out: &mut Vec<Event>, key: Key) {
    keyboard.handle(stroke(key, Kind::Press), out);
}

/// A held-gating keyboard starting at `octave`, with `low` and `high`
/// octaves of room, and `-` and `=` as its octave keys.
fn octaves(octave: i32, low: i32, high: i32) -> (Keyboard, Vec<Event>) {
    let (mut keyboard, out) = keyboard(Gating::Held, Duration::from_millis(200));
    keyboard.keymap.octave = octave;
    keyboard.keymap.octave_low = low;
    keyboard.keymap.octave_high = high;
    keyboard.keymap.octave_down = vec![Key::Char('-')];
    keyboard.keymap.octave_up = vec![Key::Char('=')];
    keyboard.octave = octave;
    (keyboard, out)
}

#[test]
fn the_scope_key_moves_the_scope_and_plays_nothing() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::from_millis(200));
    let outcome = keyboard.handle(stroke(Key::Tab, Kind::Press), &mut out);
    assert_eq!(outcome, Outcome::Scope);
    assert!(out.is_empty());
}

#[test]
fn the_start_octave_shifts_every_note() {
    let (mut keyboard, mut out) = octaves(-1, 4, 4);
    press(&mut keyboard, &mut out, Key::Char('a'));
    assert_eq!(ons(&mut out), vec![48], "60 an octave down");
    assert_eq!(keyboard.octave(), (-1, true));
}

#[test]
fn the_octave_keys_stop_at_separate_limits() {
    let (mut keyboard, mut out) = octaves(-1, 3, 4);
    for _ in 0..10 {
        press(&mut keyboard, &mut out, Key::Char('-'));
    }
    assert_eq!(keyboard.octave(), (-4, false), "three below the start");
    for _ in 0..10 {
        press(&mut keyboard, &mut out, Key::Char('='));
    }
    assert_eq!(keyboard.octave(), (3, false), "four above the start");
    assert_eq!(keyboard.reach(), (-4, 3));
}

fn release(keyboard: &mut Keyboard, out: &mut Vec<Event>, key: Key) {
    keyboard.handle(stroke(key, Kind::Release), out);
}

fn ons(out: &mut Vec<Event>) -> Vec<u8> {
    drain(out)
        .into_iter()
        .filter_map(|e| match e {
            Event::NoteOn { note, .. } => Some(note),
            _ => None,
        })
        .collect()
}

fn drain(out: &mut Vec<Event>) -> Vec<Event> {
    std::mem::take(out)
}

#[test]
fn fixed_gating_ends_every_note_in_a_chord() {
    let (mut keyboard, mut out) = keyboard(Gating::Fixed, Duration::ZERO);
    press(&mut keyboard, &mut out, Key::Char('a'));
    press(&mut keyboard, &mut out, Key::Char('s'));
    drain(&mut out);

    keyboard.tick(&mut out);
    // HashMap order, so sort before comparing
    let mut offs: Vec<u8> = drain(&mut out)
        .into_iter()
        .filter_map(|e| match e {
            Event::NoteOff { note, .. } => Some(note),
            _ => None,
        })
        .collect();
    offs.sort();
    assert_eq!(offs, vec![60, 62]);
    assert!(keyboard.held.is_empty());
}

#[test]
fn fixed_gating_waits_for_the_deadline() {
    let (mut keyboard, mut out) = keyboard(Gating::Fixed, Duration::from_secs(60));
    press(&mut keyboard, &mut out, Key::Char('a'));
    drain(&mut out);

    keyboard.tick(&mut out);
    assert!(drain(&mut out).is_empty());
}

#[test]
fn panic_releases_everything_once() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::ZERO);
    press(&mut keyboard, &mut out, Key::Char('a'));
    press(&mut keyboard, &mut out, Key::Char('s'));
    drain(&mut out);

    press(&mut keyboard, &mut out, Key::Backspace);
    assert_eq!(
        drain(&mut out),
        vec![Event::AllNotesOff {
            instrument: INSTRUMENT
        }]
    );
    assert!(keyboard.held.is_empty());
    assert_eq!(keyboard.sounding(), None);
}

const NOTES_OFF: Event = Event::AllNotesOff {
    instrument: INSTRUMENT,
};
const SOUND_OFF: Event = Event::AllSoundOff {
    instrument: INSTRUMENT,
};

// the first tap sends at once, so it is never held back to wait for a
// second; the second adds All Sound Off after the release
#[test]
fn a_double_tap_of_stop_cuts_the_sound() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::ZERO);
    press(&mut keyboard, &mut out, Key::Char('a'));
    drain(&mut out);
    let start = Instant::now();

    keyboard.stop(start, &mut out);
    assert_eq!(drain(&mut out), vec![NOTES_OFF]);
    keyboard.stop(start + DOUBLETAP / 2, &mut out);
    assert_eq!(drain(&mut out), vec![NOTES_OFF, SOUND_OFF]);
    assert!(keyboard.held.is_empty());
}

#[test]
fn taps_of_stop_apart_only_release() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::ZERO);
    let start = Instant::now();
    keyboard.stop(start, &mut out);
    keyboard.stop(start + DOUBLETAP, &mut out);
    assert_eq!(drain(&mut out), vec![NOTES_OFF, NOTES_OFF]);
}

#[test]
fn a_zero_doubletap_threshold_never_cuts_the_sound() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::ZERO);
    keyboard.keymap.doubletap = Duration::ZERO;
    let start = Instant::now();
    keyboard.stop(start, &mut out);
    keyboard.stop(start, &mut out);
    assert_eq!(drain(&mut out), vec![NOTES_OFF, NOTES_OFF]);
}

#[test]
fn held_gating_ignores_autorepeat() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::ZERO);
    for _ in 0..3 {
        press(&mut keyboard, &mut out, Key::Char('a'));
    }
    assert_eq!(ons(&mut out), vec![60]);
}

#[test]
fn held_gating_retriggers_after_release() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::ZERO);
    press(&mut keyboard, &mut out, Key::Char('a'));
    release(&mut keyboard, &mut out, Key::Char('a'));
    press(&mut keyboard, &mut out, Key::Char('a'));
    assert_eq!(ons(&mut out), vec![60, 60]);
}

#[test]
fn held_gating_plays_and_releases_shifted_keys() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::ZERO);
    press(&mut keyboard, &mut out, Key::Char('A'));
    press(&mut keyboard, &mut out, Key::Char('A'));
    release(&mut keyboard, &mut out, Key::Char('A'));
    assert_eq!(
        drain(&mut out),
        vec![
            Event::NoteOn {
                instrument: INSTRUMENT,
                note: 72,
                velocity: 100,
            },
            Event::NoteOff {
                instrument: INSTRUMENT,
                note: 72,
            },
        ]
    );
}

#[test]
fn fixed_gating_ignores_autorepeat() {
    let (mut keyboard, mut out) = keyboard(Gating::Fixed, Duration::ZERO);
    press(&mut keyboard, &mut out, Key::Char('a'));
    // the note ends before the first repeat, as with a short gate
    keyboard.tick(&mut out);
    press(&mut keyboard, &mut out, Key::Char('a'));
    assert_eq!(ons(&mut out), vec![60]);
}

#[test]
fn fixed_gating_window_follows_the_repeats() {
    let (mut keyboard, _out) = keyboard(Gating::Fixed, Duration::ZERO);
    let start = Instant::now();
    let step = REPEAT / 2;
    assert!(!keyboard.repeated('a', start));
    // each repeat restarts the window, so a long hold stays one note
    assert!(keyboard.repeated('a', start + step));
    assert!(keyboard.repeated('a', start + step * 2));
    assert!(keyboard.repeated('a', start + step * 3));
    // a press after a full window of quiet is a new keystroke
    assert!(!keyboard.repeated('a', start + step * 3 + REPEAT));
    // keys are timed separately
    assert!(!keyboard.repeated('s', start + step));
}

// the default threshold against GNOME's autorepeat, 500 ms then every
// 30: a held key sounds again at the delay and never after, while
// quarter notes at 120 bpm each sound
#[test]
fn fixed_gating_a_short_threshold_passes_fast_notes() {
    let (mut keyboard, _out) = keyboard(Gating::Fixed, Duration::ZERO);
    keyboard.keymap.autorepeat = Duration::from_millis(60);
    let ms = |n| Instant::now() + Duration::from_millis(n);

    let start = Instant::now();
    assert!(!keyboard.repeated('a', start));
    assert!(!keyboard.repeated('a', start + Duration::from_millis(500)));
    assert!(keyboard.repeated('a', start + Duration::from_millis(530)));
    assert!(keyboard.repeated('a', start + Duration::from_millis(560)));

    let beat = ms(10_000);
    for n in 0..4 {
        let at = beat + Duration::from_millis(500 * n);
        assert!(!keyboard.repeated('s', at), "quarter {n} sounds");
    }
}

#[test]
fn fixed_gating_zero_window_retriggers_every_press() {
    let (mut keyboard, mut out) = keyboard(Gating::Fixed, Duration::ZERO);
    keyboard.keymap.autorepeat = Duration::ZERO;
    press(&mut keyboard, &mut out, Key::Char('a'));
    press(&mut keyboard, &mut out, Key::Char('a'));
    assert_eq!(ons(&mut out), vec![60, 60]);
}

#[test]
fn a_ctrl_binding_needs_ctrl() {
    let (mut keyboard, mut out) = keyboard(Gating::Held, Duration::ZERO);
    let ctrl = Stroke {
        key: Key::Char('c'),
        kind: Kind::Press,
        ctrl: true,
    };
    assert_eq!(keyboard.handle(ctrl, &mut out), Outcome::Quit);
    let plain = stroke(Key::Char('c'), Kind::Press);
    assert_eq!(keyboard.handle(plain, &mut out), Outcome::Continue);
}
