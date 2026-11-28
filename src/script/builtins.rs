//! Copies of the built-in files under the custom folder: each module in
//! `synth/`, and the guides at the folder's root.
//!
//! Each folder's `.base.sha256` records the hash of the built-in each copy
//! there was written from, in `sha256sum`'s format, so
//! `sha256sum -c .base.sha256` lists the copies a user edited. A copy still
//! matching its base follows a newer built-in; an edited copy is kept, and
//! an edited module replaces the built-in.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::Error;
use sha2::{Digest, Sha256};

use super::MODULES;

/// The folder under the custom folder that holds the module copies.
const DIR: &str = "synth";
/// The record of each copy's base, beside the copies.
const BASES: &str = ".base.sha256";

/// Guides for the custom folder, written to its root.
const GUIDES: [(&str, &str); 3] = [
    ("README.md", include_str!("../../lua/custom/README.md")),
    ("AGENTS.md", include_str!("../../lua/custom/AGENTS.md")),
    (".luarc.json", include_str!("../../lua/custom/.luarc.json")),
];

/// One built-in file and where its copy goes.
struct Entry {
    name: &'static str,
    /// folder under the custom folder; empty for its root
    dir: &'static str,
    file: String,
    source: &'static str,
}

/// Every built-in with a copy: the modules, then the guides.
fn entries() -> impl Iterator<Item = Entry> {
    let modules = MODULES.iter().map(|(name, source)| Entry {
        name,
        dir: DIR,
        file: file_name(name),
        source,
    });
    let guides = GUIDES.iter().map(|(file, source)| Entry {
        name: file,
        dir: "",
        file: file.to_string(),
        source,
    });
    modules.chain(guides)
}

/// Where one built-in stands against its copy.
#[derive(Debug, PartialEq)]
pub enum State {
    /// no copy, or a copy still matching its base: the built-in stands
    BuiltIn,
    /// the copy differs from its base; a module's loads instead
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

/// The copy's file name for a module: `synth.wave` is `wave.lua`,
/// `synth.input.keyboard` is `input/keyboard.lua`, and the root module
/// `synth` is `init.lua`, as `require` would look for them.
fn file_name(name: &str) -> String {
    match name.strip_prefix("synth.") {
        Some(rest) => format!("{}.lua", rest.replace('.', "/")),
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

/// The copy that replaces built-in module `name`, if one is edited. Read
/// by the module searcher on every `require` of a `synth.*` name.
pub(super) fn edited(custom: &Path, name: &str) -> Option<(PathBuf, String)> {
    let dir = custom.join(DIR);
    let file = file_name(name);
    let path = dir.join(&file);
    let source = fs::read_to_string(&path).ok()?;
    let base = bases(&dir).remove(&file);
    (base.as_deref() != Some(hash(source.as_bytes()).as_str())).then_some((path, source))
}

/// Where every built-in stands, for `doctor`.
pub fn states(custom: &Path) -> Vec<(&'static str, State)> {
    let mut records: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
    entries()
        .map(|entry| {
            let dir = custom.join(entry.dir);
            let bases = records.entry(entry.dir).or_insert_with(|| bases(&dir));
            let path = dir.join(&entry.file);
            let state = match (fs::read(&path), bases.get(&entry.file)) {
                (Ok(copy), base) if base != Some(&hash(&copy)) => {
                    if base.is_some_and(|base| *base != hash(entry.source.as_bytes())) {
                        State::EditedBehind(path)
                    } else {
                        State::Edited(path)
                    }
                }
                _ => State::BuiltIn,
            };
            (entry.name, state)
        })
        .collect()
}

/// Writes a copy of every built-in that has none, refreshes unedited copies
/// to the current built-in, and leaves edited ones alone, then rewrites
/// each folder's `.base.sha256`.
pub fn write(custom: &Path) -> Result<Vec<(&'static str, Wrote)>, Error> {
    let mut records: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
    let mut wrote = Vec::new();

    for entry in entries() {
        let dir = custom.join(entry.dir);
        if !records.contains_key(entry.dir) {
            fs::create_dir_all(&dir).map_err(|error| Error::io("create", &dir, error))?;
            records.insert(entry.dir, bases(&dir));
        }
        let bases = records.get_mut(entry.dir).expect("inserted above");
        let path = dir.join(&entry.file);
        let built_in = hash(entry.source.as_bytes());
        let copy = match fs::read(&path) {
            Ok(copy) => Some(hash(&copy)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(Error::io("read", &path, error)),
        };
        let base = bases.get(&entry.file).cloned();

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
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| Error::io("create", parent, error))?;
            }
            fs::write(&path, entry.source).map_err(|error| Error::io("write", &path, error))?;
        }
        // a kept copy behind the built-in keeps its old base, so `doctor`
        // and the prompt to come can still tell it is behind
        if outcome != Wrote::KeptBehind {
            bases.insert(entry.file, built_in);
        }
        wrote.push((entry.name, outcome));
    }

    for (dir, bases) in &records {
        save_bases(&custom.join(dir), bases)?;
    }
    Ok(wrote)
}

fn save_bases(dir: &Path, bases: &BTreeMap<String, String>) -> Result<(), Error> {
    let record: String = bases
        .iter()
        .map(|(file, hash)| format!("{hash}  {file}\n"))
        .collect();
    let path = dir.join(BASES);
    fs::write(&path, record).map_err(|error| Error::io("write", &path, error))
}

/// Whether `synth custom` has written copies under `custom`, so there is
/// anything for `synth update` to bring up to date.
pub fn written(custom: &Path) -> bool {
    custom.join(DIR).join(BASES).exists()
}

/// An edited copy whose built-in has changed since it was written: the one
/// case `synth custom` asks about.
pub struct Behind {
    pub name: &'static str,
    pub path: PathBuf,
    pub copy: String,
    pub built_in: &'static str,
    dir: &'static str,
    file: String,
}

/// The copy of `name` and its new built-in, for the prompt, if the copy is
/// behind.
pub fn behind(custom: &Path, name: &str) -> Result<Option<Behind>, Error> {
    let Some(entry) = entries().find(|entry| entry.name == name) else {
        return Ok(None);
    };
    if !matches!(
        states(custom).into_iter().find(|(n, _)| *n == name),
        Some((_, State::EditedBehind(_)))
    ) {
        return Ok(None);
    }
    let path = custom.join(entry.dir).join(&entry.file);
    let copy = fs::read_to_string(&path).map_err(|error| Error::io("read", &path, error))?;
    Ok(Some(Behind {
        name: entry.name,
        path,
        copy,
        built_in: entry.source,
        dir: entry.dir,
        file: entry.file,
    }))
}

/// Replaces the copy with the current built-in, as the user chose, and
/// records it as unedited.
pub fn overwrite(custom: &Path, behind: &Behind) -> Result<(), Error> {
    fs::write(&behind.path, behind.built_in)
        .map_err(|error| Error::io("write", &behind.path, error))?;
    rebase(custom, behind)
}

/// Keeps the copy, as the user chose, and records the current built-in as
/// its base, so it counts as edited but no longer behind until the
/// built-in changes again.
pub fn keep(custom: &Path, behind: &Behind) -> Result<(), Error> {
    rebase(custom, behind)
}

fn rebase(custom: &Path, behind: &Behind) -> Result<(), Error> {
    let dir = custom.join(behind.dir);
    let mut bases = bases(&dir);
    bases.insert(behind.file.clone(), hash(behind.built_in.as_bytes()));
    save_bases(&dir, &bases)
}

#[cfg(test)]
mod tests;
