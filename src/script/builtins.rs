//! Editable copies of the built-in modules, in `synth/` under the patches folder.
//!
//! `.base.sha256` records the hash of the built-in each copy was written
//! from, in `sha256sum`'s format, so `sha256sum -c .base.sha256` lists the
//! copies a user edited. A copy still matching its base is not used, so a
//! newer built-in reaches the user; an edited copy replaces the built-in.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use sha2::{Digest, Sha256};

use super::MODULES;

/// The folder under the patches folder that holds the copies.
const DIR: &str = "synth";
/// The record of each copy's base, beside the copies.
const BASES: &str = ".base.sha256";

/// Where one built-in stands against its copy.
#[derive(Debug, PartialEq)]
pub enum State {
    /// no copy, or a copy still matching its base: the built-in loads
    BuiltIn,
    /// the copy differs from its base, so it loads instead of the built-in
    Edited(PathBuf),
    /// edited, and the built-in has changed since the copy was written
    EditedBehind(PathBuf),
}

/// What writing the copies did to one file.
#[derive(Debug, PartialEq)]
pub enum Wrote {
    /// there was no copy
    Written,
    /// an unedited copy was replaced by a newer built-in
    Refreshed,
    /// nothing to do
    Current,
    /// an edited copy, left alone
    Kept,
    /// an edited copy, left alone though the built-in has changed since
    KeptBehind,
}

/// The copy's file name for a module: `synth.wave` is `wave.lua`, and the
/// root module `synth` is `init.lua`, as `require` would look for them.
fn file_name(name: &str) -> String {
    match name.strip_prefix("synth.") {
        Some(rest) => format!("{rest}.lua"),
        None => "init.lua".to_string(),
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `.base.sha256` as file name to hash. A missing or unreadable record is
/// empty, which makes every existing copy count as edited: nothing a user
/// wrote is overwritten for want of a record.
fn bases(dir: &Path) -> BTreeMap<String, String> {
    let Ok(text) = fs::read_to_string(dir.join(BASES)) else {
        return BTreeMap::new();
    };
    text.lines()
        .filter_map(|line| {
            let (hash, file) = line.split_once("  ")?;
            Some((file.to_string(), hash.to_string()))
        })
        .collect()
}

/// The copy that replaces built-in `name`, if one is edited. Read by the
/// module searcher on every `require` of a `synth.*` name.
pub(super) fn edited(patches: &Path, name: &str) -> Option<(PathBuf, String)> {
    let dir = patches.join(DIR);
    let file = file_name(name);
    let path = dir.join(&file);
    let source = fs::read_to_string(&path).ok()?;
    let base = bases(&dir).remove(&file);
    (base.as_deref() != Some(hash(source.as_bytes()).as_str())).then_some((path, source))
}

/// Where every built-in stands, for `doctor`.
pub fn states(patches: &Path) -> Vec<(&'static str, State)> {
    let dir = patches.join(DIR);
    let bases = bases(&dir);
    MODULES
        .iter()
        .map(|(name, source)| {
            let file = file_name(name);
            let path = dir.join(&file);
            let state = match (fs::read(&path), bases.get(&file)) {
                (Ok(copy), base) if base != Some(&hash(&copy)) => {
                    if base.is_some_and(|base| *base != hash(source.as_bytes())) {
                        State::EditedBehind(path)
                    } else {
                        State::Edited(path)
                    }
                }
                _ => State::BuiltIn,
            };
            (*name, state)
        })
        .collect()
}

/// Writes a copy of every built-in that has none, refreshes unedited copies
/// to the current built-in, and leaves edited ones alone, then rewrites
/// `.base.sha256`.
pub fn write(patches: &Path) -> anyhow::Result<Vec<(&'static str, Wrote)>> {
    let dir = patches.join(DIR);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let mut bases = bases(&dir);
    let mut wrote = Vec::new();

    for (name, source) in MODULES {
        let file = file_name(name);
        let path = dir.join(&file);
        let built_in = hash(source.as_bytes());
        let copy = match fs::read(&path) {
            Ok(copy) => Some(hash(&copy)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        let base = bases.get(&file).cloned();

        let outcome = match (copy, base) {
            (None, _) => Wrote::Written,
            (Some(copy), Some(base)) if copy == base && base == built_in => Wrote::Current,
            (Some(copy), Some(base)) if copy == base => Wrote::Refreshed,
            // edited, against a base the built-in still matches
            (Some(_), Some(base)) if base == built_in => Wrote::Kept,
            (Some(_), Some(_)) => Wrote::KeptBehind,
            // a copy with no record: someone else's file, so it is kept,
            // and recorded against today's built-in
            (Some(copy), None) if copy == built_in => Wrote::Current,
            (Some(_), None) => Wrote::Kept,
        };
        if matches!(outcome, Wrote::Written | Wrote::Refreshed) {
            fs::write(&path, source).with_context(|| format!("write {}", path.display()))?;
        }
        // a kept copy behind the built-in keeps its old base, so `doctor`
        // and the prompt to come can still tell it is behind
        if outcome != Wrote::KeptBehind {
            bases.insert(file, built_in);
        }
        wrote.push((name, outcome));
    }

    save_bases(&dir, &bases)?;
    Ok(wrote)
}

fn save_bases(dir: &Path, bases: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let record: String = bases
        .iter()
        .map(|(file, hash)| format!("{hash}  {file}\n"))
        .collect();
    let path = dir.join(BASES);
    fs::write(&path, record).with_context(|| format!("write {}", path.display()))
}

/// Whether `synth patches` has written copies under `patches`, so there is
/// anything for `synth update` to bring up to date.
pub fn written(patches: &Path) -> bool {
    patches.join(DIR).join(BASES).exists()
}

/// An edited copy whose built-in has changed since it was written: the one
/// case `synth patches` asks about.
pub struct Behind {
    pub name: &'static str,
    pub path: PathBuf,
    pub copy: String,
    pub built_in: &'static str,
}

/// The copy of `name` and its new built-in, for the prompt, if the copy is
/// behind.
pub fn behind(patches: &Path, name: &str) -> anyhow::Result<Option<Behind>> {
    let Some((name, source)) = MODULES.iter().find(|(n, _)| *n == name) else {
        return Ok(None);
    };
    let path = patches.join(DIR).join(file_name(name));
    if !matches!(
        states(patches).into_iter().find(|(n, _)| n == name),
        Some((_, State::EditedBehind(_)))
    ) {
        return Ok(None);
    }
    let copy = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(Some(Behind {
        name,
        path,
        copy,
        built_in: source,
    }))
}

/// Replaces the copy with the current built-in, as the user chose, and
/// records it as unedited.
pub fn overwrite(patches: &Path, behind: &Behind) -> anyhow::Result<()> {
    fs::write(&behind.path, behind.built_in)
        .with_context(|| format!("write {}", behind.path.display()))?;
    rebase(patches, behind)
}

/// Keeps the copy, as the user chose, and records the current built-in as
/// its base, so it counts as edited but no longer behind until the
/// built-in changes again.
pub fn keep(patches: &Path, behind: &Behind) -> anyhow::Result<()> {
    rebase(patches, behind)
}

fn rebase(patches: &Path, behind: &Behind) -> anyhow::Result<()> {
    let dir = patches.join(DIR);
    let mut bases = bases(&dir);
    bases.insert(file_name(behind.name), hash(behind.built_in.as_bytes()));
    save_bases(&dir, &bases)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty patches folder for one test.
    fn patches(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("synth-builtins-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wave(patches: &Path) -> PathBuf {
        patches.join(DIR).join("wave.lua")
    }

    fn outcome(wrote: &[(&str, Wrote)], name: &str) -> &'static str {
        match wrote.iter().find(|(n, _)| *n == name).map(|(_, w)| w) {
            Some(Wrote::Written) => "written",
            Some(Wrote::Refreshed) => "refreshed",
            Some(Wrote::Current) => "current",
            Some(Wrote::Kept) => "kept",
            Some(Wrote::KeptBehind) => "kept behind",
            None => "missing",
        }
    }

    /// Records `base` as wave.lua's base, as an older synth would have.
    fn set_base(patches: &Path, base: &[u8]) {
        let path = patches.join(DIR).join(BASES);
        let text = fs::read_to_string(&path).unwrap();
        let text: String = text
            .lines()
            .map(|line| match line.ends_with("  wave.lua") {
                true => format!("{}  wave.lua\n", hash(base)),
                false => format!("{line}\n"),
            })
            .collect();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_first_write_copies_every_built_in_unedited() {
        let dir = patches("first");
        let wrote = write(&dir).unwrap();
        assert!(wrote.iter().all(|(_, w)| *w == Wrote::Written), "{wrote:?}");
        assert_eq!(wrote.len(), MODULES.len());
        let record = fs::read_to_string(dir.join(DIR).join(BASES)).unwrap();
        assert_eq!(record.lines().count(), MODULES.len());
        assert!(states(&dir).iter().all(|(_, s)| *s == State::BuiltIn));
        assert!(write(&dir)
            .unwrap()
            .iter()
            .all(|(_, w)| *w == Wrote::Current));
    }

    #[test]
    fn an_edited_copy_replaces_the_built_in_and_is_kept() {
        let dir = patches("edited");
        write(&dir).unwrap();
        fs::write(wave(&dir), "return { edited = true }").unwrap();

        assert!(edited(&dir, "synth.wave").is_some());
        assert_eq!(
            states(&dir)
                .into_iter()
                .find(|(n, _)| *n == "synth.wave")
                .unwrap()
                .1,
            State::Edited(wave(&dir))
        );
        assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "kept");
        assert_eq!(
            fs::read_to_string(wave(&dir)).unwrap(),
            "return { edited = true }"
        );
    }

    #[test]
    fn an_unedited_copy_follows_a_newer_built_in() {
        let dir = patches("refresh");
        write(&dir).unwrap();
        // as if an older synth wrote this copy, unedited since
        fs::write(wave(&dir), "old").unwrap();
        set_base(&dir, b"old");

        assert!(
            edited(&dir, "synth.wave").is_none(),
            "unedited, so not used"
        );
        assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "refreshed");
        let built_in = MODULES.iter().find(|(n, _)| *n == "synth.wave").unwrap().1;
        assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), built_in);
    }

    #[test]
    fn an_edited_copy_behind_a_newer_built_in_is_kept_and_named() {
        let dir = patches("behind");
        write(&dir).unwrap();
        fs::write(wave(&dir), "mine").unwrap();
        set_base(&dir, b"old");

        assert_eq!(
            states(&dir)
                .into_iter()
                .find(|(n, _)| *n == "synth.wave")
                .unwrap()
                .1,
            State::EditedBehind(wave(&dir))
        );
        assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "kept behind");
        assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), "mine");
        // still behind, so the next run still knows
        assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "kept behind");
    }

    #[test]
    fn overwrite_takes_the_new_built_in_and_keep_stops_asking() {
        let dir = patches("resolve");
        write(&dir).unwrap();
        fs::write(wave(&dir), "mine").unwrap();
        set_base(&dir, b"old");

        let stale = behind(&dir, "synth.wave").unwrap().expect("behind");
        assert_eq!(stale.copy, "mine");
        keep(&dir, &stale).unwrap();
        assert!(
            behind(&dir, "synth.wave").unwrap().is_none(),
            "kept, so not asked again"
        );
        assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), "mine");
        assert!(
            edited(&dir, "synth.wave").is_some(),
            "still the user's edit"
        );

        set_base(&dir, b"old");
        let stale = behind(&dir, "synth.wave").unwrap().expect("behind again");
        overwrite(&dir, &stale).unwrap();
        assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), stale.built_in);
        assert!(edited(&dir, "synth.wave").is_none(), "now unedited");
    }

    #[test]
    fn a_copy_with_no_record_is_kept() {
        let dir = patches("unrecorded");
        fs::create_dir_all(dir.join(DIR)).unwrap();
        fs::write(wave(&dir), "someone's own").unwrap();

        assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "kept");
        assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), "someone's own");
    }

    #[test]
    fn the_root_module_is_init_lua() {
        assert_eq!(file_name("synth"), "init.lua");
        assert_eq!(file_name("synth.keyboard"), "keyboard.lua");
    }
}
