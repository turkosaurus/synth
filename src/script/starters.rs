//! One example patch per kind, written into an empty kind folder so a new
//! custom folder opens with something to copy. The files are the user's
//! once written: a folder holding any `.lua` file gets nothing.

use std::fs;
use std::path::Path;

use super::Error;

/// The file each kind folder starts with.
const FILE: &str = "example.lua";

/// Every kind's starter, by the folder it goes in.
const STARTERS: [(&str, &str); 5] = [
    (
        "instruments",
        include_str!("../../lua/custom/instruments/example.lua"),
    ),
    (
        "effects",
        include_str!("../../lua/custom/effects/example.lua"),
    ),
    (
        "keyboards",
        include_str!("../../lua/custom/keyboards/example.lua"),
    ),
    (
        "tunings",
        include_str!("../../lua/custom/tunings/example.lua"),
    ),
    (
        "patterns",
        include_str!("../../lua/custom/patterns/example.lua"),
    ),
];

fn has_lua(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().extension().is_some_and(|ext| ext == "lua"))
    })
}

/// Creates each kind folder under `custom` and writes its starter into
/// those holding no Lua yet. Returns what it wrote, as `kind/example.lua`.
pub fn write(custom: &Path) -> Result<Vec<String>, Error> {
    let mut wrote = Vec::new();
    for (kind, source) in STARTERS {
        let dir = custom.join(kind);
        fs::create_dir_all(&dir).map_err(|error| Error::io("create", &dir, error))?;
        if has_lua(&dir) {
            continue;
        }
        let path = dir.join(FILE);
        fs::write(&path, source).map_err(|error| Error::io("write", &path, error))?;
        wrote.push(format!("{kind}/{FILE}"));
    }
    Ok(wrote)
}

#[cfg(test)]
mod tests;
