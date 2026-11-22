//! One example patch per kind, written into an empty kind folder so a new
//! patches folder opens with something to copy. The files are the user's
//! once written: a folder holding any `.lua` file gets nothing.

use std::fs;
use std::path::Path;

use super::Error;

/// The file each kind folder starts with.
const FILE: &str = "example.lua";

/// Every kind's starter, by the folder it goes in.
const STARTERS: [(&str, &str); 5] = [
    (
        "instrument",
        include_str!("../../lua/patches/instrument/example.lua"),
    ),
    (
        "effect",
        include_str!("../../lua/patches/effect/example.lua"),
    ),
    (
        "keyboard",
        include_str!("../../lua/patches/keyboard/example.lua"),
    ),
    (
        "tuning",
        include_str!("../../lua/patches/tuning/example.lua"),
    ),
    (
        "pattern",
        include_str!("../../lua/patches/pattern/example.lua"),
    ),
];

fn has_lua(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().extension().is_some_and(|ext| ext == "lua"))
    })
}

/// Creates each kind folder under `patches` and writes its starter into
/// those holding no Lua yet. Returns what it wrote, as `kind/example.lua`.
pub fn write(patches: &Path) -> Result<Vec<String>, Error> {
    let mut wrote = Vec::new();
    for (kind, source) in STARTERS {
        let dir = patches.join(kind);
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

    fn patches(test: &str) -> std::path::PathBuf {
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
        let dir = patches("once");
        assert_eq!(write(&dir).unwrap().len(), STARTERS.len());
        assert!(dir.join("pattern").join(FILE).exists());
        assert!(write(&dir).unwrap().is_empty(), "already there");
    }

    #[test]
    fn a_folder_with_a_patch_of_its_own_gets_none() {
        let dir = patches("own");
        fs::create_dir_all(dir.join("instrument")).unwrap();
        fs::write(dir.join("instrument").join("bass.lua"), "return {}").unwrap();
        let wrote = write(&dir).unwrap();
        assert!(!wrote.contains(&"instrument/example.lua".to_string()));
        assert!(!dir.join("instrument").join(FILE).exists());
        // a deleted starter stays deleted while the folder has other patches
        fs::remove_file(dir.join("tuning").join(FILE)).unwrap();
        fs::write(dir.join("tuning").join("just.lua"), "return {}").unwrap();
        assert!(write(&dir).unwrap().is_empty());
    }
}
