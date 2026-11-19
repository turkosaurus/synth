use std::collections::HashMap;

use anyhow::Context;

use crate::spec::{Key, Keymap};

/// The raw keyboard layout as written in Lua. Field names live here, in
/// the same snake_case the Lua side uses.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyboardConfig {
    layout: Layout,
    octave_up: Vec<String>,
    octave_down: Vec<String>,
    stop: Vec<String>,
    help: Vec<String>,
    // ms, implied rather than spelled in the Lua name
    #[serde(rename = "gate")]
    gate_ms: f64,
    #[serde(rename = "repeat_window")]
    repeat_ms: f64,
    velocity: u8,
    octave_range: i32,
}

/// Which character plays which note.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    /// Absent when a patch writes its own `notes` without naming them.
    #[serde(default)]
    name: Option<String>,
    /// Character to MIDI note. A float survives here so the range check
    /// below can name it, rather than serde rejecting it unnamed.
    notes: HashMap<String, f64>,
}

pub(super) fn keymap(lua: &mlua::Lua, table: &mlua::Table) -> anyhow::Result<Keymap> {
    super::schema::KEYBOARD.check(table, None)?;
    let config: KeyboardConfig =
        mlua::LuaSerdeExt::from_value(lua, mlua::Value::Table(table.clone()))
            .context("keyboard layout")?;

    let notes = notes(config.layout.notes)?;

    // a binding is a character or a key name; an unrecognised name is an
    // error rather than a silently dead key
    let bindings = |list: Vec<String>, what: &str| -> anyhow::Result<Vec<_>> {
        list.iter()
            .map(|name| {
                binding(name).with_context(|| format!("keyboard.{what}: unknown key '{name}'"))
            })
            .collect()
    };

    let stop = bindings(config.stop, "stop")?;
    // a live instrument with no way to silence a stuck note is a trap, and
    // quitting is the only other recovery
    anyhow::ensure!(
        !stop.is_empty(),
        "keyboard.stop must name at least one key to silence with"
    );

    let octave_up = bindings(config.octave_up, "octave_up")?;
    let octave_down = bindings(config.octave_down, "octave_down")?;
    // may be empty: a layout that would rather not spend a key on it
    // just leaves help unreachable
    let help = bindings(config.help, "help")?;

    // a control is checked before a note, so a key bound to both never
    // plays its note
    for (what, keys) in [
        ("octave_up", &octave_up),
        ("octave_down", &octave_down),
        ("stop", &stop),
        ("help", &help),
    ] {
        for key in keys {
            if let Key::Char(ch) = key {
                if let Some(note) = notes.get(ch) {
                    anyhow::bail!(
                        "keyboard.layout plays note {note} on '{ch}', \
                         which keyboard.{what} also binds"
                    );
                }
            }
        }
    }

    // gate, repeat_window, velocity, and octave_range ranges: schema::KEYBOARD

    Ok(Keymap {
        notes,
        name: config.layout.name,
        octave_up,
        octave_down,
        stop,
        help,
        gate: std::time::Duration::from_secs_f64(config.gate_ms / 1000.0),
        repeat: std::time::Duration::from_secs_f64(config.repeat_ms / 1000.0),
        velocity: config.velocity,
        octave_range: config.octave_range,
    })
}

/// Checks each entry of `notes`: one character to one MIDI note. A
/// key event carries a single character, so a longer string could never
/// be pressed.
fn notes(raw: HashMap<String, f64>) -> anyhow::Result<HashMap<char, u8>> {
    let mut notes = HashMap::with_capacity(raw.len());
    for (key, note) in raw {
        let mut chars = key.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            anyhow::bail!("keyboard.layout.notes: key '{key}' must be one character");
        };
        anyhow::ensure!(
            note.fract() == 0.0 && (0.0..=127.0).contains(&note),
            "keyboard.layout.notes['{key}'] is {note}; must be a whole number 0 to 127"
        );
        notes.insert(ch, note as u8);
    }
    Ok(notes)
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
