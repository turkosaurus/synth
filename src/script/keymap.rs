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
}

/// The physical grid, kept nested because rows and their shifted forms are
/// one unit: a template is a layout, not two parallel lists.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
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

    Ok(crate::input::keyboard::Keymap {
        notes,
        octave_up: bindings(config.octave_up, "octaveUp")?,
        octave_down: bindings(config.octave_down, "octaveDown")?,
        stop,
    })
}
