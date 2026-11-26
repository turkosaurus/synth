use std::collections::HashMap;
use std::time::Duration;

use super::Error;
use crate::spec::{Key, Keymap};

/// The raw keyboard layout as written in Lua. Field names live here, in
/// the same snake_case the Lua side uses.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyboardConfig {
    layout: Layout,
    stop: Vec<String>,
    // ms, implied rather than spelled in the Lua name
    #[serde(rename = "gate")]
    gate_ms: f64,
    #[serde(rename = "autorepeat_threshold")]
    autorepeat_ms: f64,
    #[serde(rename = "doubletap_threshold")]
    doubletap_ms: f64,
    velocity: u8,
    octave: i32,
    octave_low: i32,
    octave_high: i32,
    #[serde(rename = "preview")]
    preview_ms: f64,
}

/// Which character plays which note, and which keys a layout keeps for
/// control, since a key one layout spends on a note another spends on an
/// octave.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    /// Absent when a patch writes its own `notes` without naming them.
    #[serde(default)]
    name: Option<String>,
    /// Character to MIDI note. A float survives here so the range check
    /// below can name it, rather than serde rejecting it unnamed.
    notes: HashMap<String, f64>,
    /// The anchor key's note. Absent when a patch writes its own `notes`.
    #[serde(default)]
    note: Option<f64>,
    octave_up: Vec<String>,
    octave_down: Vec<String>,
    help: Vec<String>,
}

/// What a bare instrument file plays: the layout's anchor note, if it
/// names one, held for the keyboard's `preview`.
pub(super) struct Preview {
    pub(super) note: Option<u8>,
    pub(super) hold: Duration,
}

pub(super) fn keymap(lua: &mlua::Lua, table: &mlua::Table) -> Result<(Keymap, Preview), Error> {
    super::schema::KEYBOARD.check(table, None)?;
    let config: KeyboardConfig =
        mlua::LuaSerdeExt::from_value(lua, mlua::Value::Table(table.clone())).map_err(
            |source| Error::Convert {
                what: "keyboard layout",
                source,
            },
        )?;

    let notes = notes(config.layout.notes)?;
    let note = config
        .layout
        .note
        .map(|value| midi(value).ok_or(Error::AnchorRange { value }))
        .transpose()?;

    // a binding is a character or a key name; an unrecognised name is an
    // error rather than a silently dead key
    let bindings = |list: Vec<String>, what: &str| -> Result<Vec<_>, Error> {
        list.into_iter()
            .map(|name| {
                binding(&name).ok_or_else(|| Error::UnknownKey {
                    what: format!("keyboard.{what}"),
                    key: name,
                })
            })
            .collect()
    };

    let stop = bindings(config.stop, "stop")?;
    // a live instrument with no way to silence a stuck note is a trap, and
    // quitting is the only other recovery
    if stop.is_empty() {
        return Err(Error::NoStopKey);
    }

    let octave_up = bindings(config.layout.octave_up, "layout.octave_up")?;
    let octave_down = bindings(config.layout.octave_down, "layout.octave_down")?;
    // may be empty: a layout that would rather not spend a key on it
    // just leaves help unreachable
    let help = bindings(config.layout.help, "layout.help")?;

    // a control is checked before a note, so a key bound to both never
    // plays its note
    for (what, keys) in [
        ("layout.octave_up", &octave_up),
        ("layout.octave_down", &octave_down),
        ("stop", &stop),
        ("layout.help", &help),
    ] {
        for key in keys {
            if let Key::Char(ch) = key {
                if let Some(&note) = notes.get(ch) {
                    return Err(Error::KeyConflict {
                        what,
                        key: *ch,
                        note,
                    });
                }
            }
        }
    }

    // gate, both thresholds, velocity, and octave ranges: schema::KEYBOARD

    let keymap = Keymap {
        notes,
        name: config.layout.name,
        octave_up,
        octave_down,
        stop,
        help,
        gate: std::time::Duration::from_secs_f64(config.gate_ms / 1000.0),
        autorepeat: std::time::Duration::from_secs_f64(config.autorepeat_ms / 1000.0),
        doubletap: std::time::Duration::from_secs_f64(config.doubletap_ms / 1000.0),
        velocity: config.velocity,
        octave: config.octave,
        octave_low: config.octave_low,
        octave_high: config.octave_high,
    };
    let preview = Preview {
        note,
        hold: Duration::from_secs_f64(config.preview_ms / 1000.0),
    };
    Ok((keymap, preview))
}

/// Checks each entry of `notes`: one character to one MIDI note. A
/// key event carries a single character, so a longer string could never
/// be pressed.
fn notes(raw: HashMap<String, f64>) -> Result<HashMap<char, u8>, Error> {
    let mut notes = HashMap::with_capacity(raw.len());
    for (key, note) in raw {
        let mut chars = key.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return Err(Error::NoteKey { key });
        };
        let note = midi(note).ok_or(Error::NoteRange { key, value: note })?;
        notes.insert(ch, note);
    }
    Ok(notes)
}

fn midi(value: f64) -> Option<u8> {
    (value.fract() == 0.0 && (0.0..=127.0).contains(&value)).then_some(value as u8)
}

/// Parses a binding from the keymap. A single character is that key; anything
/// longer is a name, because keys like backspace have no character to write.
fn binding(name: &str) -> Option<Key> {
    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return Some(Key::Char(ch));
    }
    Some(match name.to_ascii_lowercase().as_str() {
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "space" => Key::Char(' '),
        "esc" | "escape" => Key::Esc,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        _ => return None,
    })
}
