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
mod tests {
    use super::*;

    fn custom(test: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("synth-starters-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn every_kind_has_a_starter() {
        let kinds: Vec<&str> = STARTERS.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(kinds, super::super::PATCH_KINDS);
    }

    #[test]
    fn an_empty_folder_gets_its_starter_once() {
        let dir = custom("once");
        assert_eq!(write(&dir).unwrap().len(), STARTERS.len());
        assert!(dir.join("patterns").join(FILE).exists());
        assert!(write(&dir).unwrap().is_empty(), "already there");
    }

    #[test]
    fn a_folder_with_a_patch_of_its_own_gets_none() {
        let dir = custom("own");
        fs::create_dir_all(dir.join("instruments")).unwrap();
        fs::write(dir.join("instruments").join("bass.lua"), "return {}").unwrap();
        let wrote = write(&dir).unwrap();
        assert!(!wrote.contains(&"instruments/example.lua".to_string()));
        assert!(!dir.join("instruments").join(FILE).exists());
        // a deleted starter stays deleted while the folder has other patches
        fs::remove_file(dir.join("tunings").join(FILE)).unwrap();
        fs::write(dir.join("tunings").join("just.lua"), "return {}").unwrap();
        assert!(write(&dir).unwrap().is_empty());
    }
}
