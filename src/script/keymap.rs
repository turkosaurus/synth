use anyhow::Context;

/// The raw keyboard layout as written in Lua. Field names live here, and
/// camelCase renaming keeps the Lua side idiomatic without string literals.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KeyboardConfig {
    anchor: Anchor,
    offset: i32,
    template: Template,
    octave_up: Vec<String>,
    octave_down: Vec<String>,
    stop: Vec<String>,
    help: Vec<String>,
    // camelCase alone would make this `gateMs`; the Lua side spells unit
    // suffixes in capitals, as envelope's `attackMS` does
    #[serde(rename = "gateMS")]
    gate_ms: f64,
    velocity: u8,
    octave_range: i32,
}

/// The physical grid, kept nested because rows and their shifted forms are
/// one unit: a template is a layout, not two parallel lists.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    /// Absent when a patch supplies its own rows rather than naming one of
    /// the built-in layouts.
    #[serde(default)]
    name: Option<String>,
    rows: Vec<String>,
    shifted: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    key: String,
    note: i32,
}

pub(super) fn keymap(
    lua: &mlua::Lua,
    table: &mlua::Table,
) -> anyhow::Result<crate::input::keyboard::Keymap> {
    let config: KeyboardConfig =
        mlua::LuaSerdeExt::from_value(lua, mlua::Value::Table(table.clone()))
            .context("keyboard layout")?;

    anyhow::ensure!(
        config.template.rows.len() == config.template.shifted.len(),
        "a template's rows and shifted must have the same number of rows"
    );
    let anchor_char = config
        .anchor
        .key
        .chars()
        .next()
        .context("keyboard.anchor.key is empty")?;

    // the anchor fixes the grid, so changing `offset` keeps that key on pitch
    let mut base = None;
    for (r, row) in config.template.rows.iter().enumerate() {
        if let Some(c) = row.chars().position(|ch| ch == anchor_char) {
            base = Some(config.anchor.note - r as i32 * config.offset - c as i32);
        }
    }
    let base = base.with_context(|| {
        format!("keyboard.anchor.key '{anchor_char}' does not appear in the template")
    })?;

    let mut notes = std::collections::HashMap::new();
    for (r, (row, up)) in config
        .template
        .rows
        .iter()
        .zip(&config.template.shifted)
        .enumerate()
    {
        anyhow::ensure!(
            row.chars().count() == up.chars().count(),
            "template row {} and its shifted form differ in length",
            r + 1
        );
        for (c, (plain, raised)) in row.chars().zip(up.chars()).enumerate() {
            let note = base + r as i32 * config.offset + c as i32;
            if (0..128).contains(&note) {
                notes.insert(plain, note as u8);
            }
            // the shifted form of a key is the same position, one octave up
            if (0..128).contains(&(note + 12)) {
                notes.insert(raised, (note + 12) as u8);
            }
        }
    }

    // a binding is a character or a key name; an unrecognised name is an
    // error rather than a silently dead key
    let bindings = |list: Vec<String>, what: &str| -> anyhow::Result<Vec<_>> {
        list.iter()
            .map(|name| {
                crate::input::keyboard::binding(name)
                    .with_context(|| format!("keyboard.{what}: unknown key '{name}'"))
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

    // a zero gate would end each note on the poll after it starts
    anyhow::ensure!(
        config.gate_ms.is_finite() && config.gate_ms > 0.0,
        "keyboard.gateMS must be a positive number of milliseconds, got {}",
        config.gate_ms
    );

    // 0 is MIDI's note-off spelled as a note-on, and above 127 is not MIDI
    anyhow::ensure!(
        (1..=127).contains(&config.velocity),
        "keyboard.velocity must be 1 to 127, got {}",
        config.velocity
    );

    // past 10 octaves every shift leaves MIDI's 0..127 note range
    anyhow::ensure!(
        (0..=10).contains(&config.octave_range),
        "keyboard.octaveRange must be 0 to 10, got {}",
        config.octave_range
    );

    Ok(crate::input::keyboard::Keymap {
        notes,
        template: config.template.name,
        offset: config.offset,
        octave_up: bindings(config.octave_up, "octaveUp")?,
        octave_down: bindings(config.octave_down, "octaveDown")?,
        stop,
        // may be empty: a layout that would rather not spend a key on it
        // just leaves help unreachable
        help: bindings(config.help, "help")?,
        gate: std::time::Duration::from_secs_f64(config.gate_ms / 1000.0),
        velocity: config.velocity,
        octave_range: config.octave_range,
    })
}
