//! The shape of a patch as its constructors built it.
//!
//! Every constructor stamps the table it returns with its kind and the line
//! that called it, on the table's metatable so `pairs()` never sees them.
//! The walk reads those stamps from the root down: it checks that each field
//! holds the kind it should, rejects cycles, and records every stamped table
//! by its path.

use super::{field, Error};
use crate::spec::Node;

/// What a constructor wrote on a table's metatable.
struct Stamp {
    kind: String,
    at: String,
}

fn stamp(table: &mlua::Table) -> mlua::Result<Option<Stamp>> {
    let Some(meta) = table.metatable() else {
        return Ok(None);
    };
    let kind: Option<String> = meta.get(field::STAMP_KIND)?;
    let at: Option<String> = meta.get(field::STAMP_AT)?;
    Ok(kind.map(|kind| Stamp {
        kind,
        at: at.unwrap_or_default(),
    }))
}

/// Where a constructor said it made `table`, if one stamped it.
pub(super) fn made_at(table: &mlua::Table) -> mlua::Result<Option<String>> {
    Ok(stamp(table)?.map(|found| found.at))
}

/// What a field must hold, decided by the kind of the table holding it.
enum Expect {
    One(&'static str),
    /// a table of named values, each of this kind
    MapOf(&'static str),
    /// a plain table whose named fields each hold their own kind
    Fields(&'static [(&'static str, &'static str)]),
}

fn expected(parent: &str, key: &str) -> Option<Expect> {
    Some(match (parent, key) {
        ("instrument", "envelope") => Expect::One("envelope"),
        ("instrument", "oscillator") => Expect::One("osc"),
        ("osc", "position_mod") => Expect::One("lfo"),
        ("filter", "cutoff_mod") => Expect::One("lfo"),
        ("instrument" | "sequence", "input") => Expect::Fields(&[("keyboard", "keyboard")]),
        ("instrument" | "sequence", "filter") => Expect::One("filter"),
        ("sequence", "instruments") => Expect::MapOf("instrument"),
        _ => return None,
    })
}

/// `instruments.tri.envelope`, or `the patch` for the root.
fn name(path: &str) -> &str {
    if path.is_empty() {
        "the patch"
    } else {
        path
    }
}

fn check(table: &mlua::Table, kind: &'static str, path: &str) -> Result<(), Error> {
    match stamp(table)? {
        Some(found) if found.kind == kind => Ok(()),
        Some(found) => Err(Error::WrongKind {
            what: name(path).to_string(),
            expected: kind,
            got: found.kind,
            at: found.at,
        }),
        None => Err(Error::PlainTable {
            what: name(path).to_string(),
            expected: kind,
        }),
    }
}

/// Fields an instrument may hold only as the patch's root, which reads them.
/// Anywhere else they would load and be ignored.
const ROOT_ONLY: [&str; 3] = [field::INPUT, field::TUNING, field::FILTER];

fn root_only(table: &mlua::Table, path: &str, at: &str) -> Result<(), Error> {
    for key in ROOT_ONLY {
        if !table.raw_get::<mlua::Value>(key)?.is_nil() {
            return Err(Error::RootOnly {
                what: path.to_string(),
                field: key,
                at: at.to_string(),
            });
        }
    }
    Ok(())
}

/// Walks the patch from its root and returns every stamped table, sorted by
/// path. Fails on a root that is not an instrument or a sequence, on a field
/// holding the wrong kind, and on a cycle.
pub(super) fn walk(root: &mlua::Table, source: &str) -> Result<Vec<Node>, Error> {
    match stamp(root)? {
        Some(found) if matches!(found.kind.as_str(), "instrument" | "sequence") => {}
        Some(found) => {
            return Err(Error::RootKind {
                got: found.kind,
                at: found.at,
            })
        }
        None => return Err(Error::RootPlainTable),
    }
    let mut nodes = Vec::new();
    visit(root, String::new(), &mut Vec::new(), &mut nodes)?;
    // `return synth.sequence({...})` is a tail call, which drops the patch's
    // own frame before the constructor runs, so the root names its file
    if let Some(root) = nodes.iter_mut().find(|node| node.path.is_empty()) {
        if root.at == super::BUILT_IN {
            root.at = source.to_string();
        }
    }
    nodes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(nodes)
}

fn visit(
    table: &mlua::Table,
    path: String,
    ancestors: &mut Vec<*const std::ffi::c_void>,
    nodes: &mut Vec<Node>,
) -> Result<(), Error> {
    // only ancestors count: a table shared by two fields is fine, one that
    // contains itself is not
    let here = table.to_pointer();
    if ancestors.contains(&here) {
        return Err(Error::Cycle {
            what: name(&path).to_string(),
        });
    }

    let found = stamp(table)?;
    if let Some(found) = &found {
        if found.kind == "instrument" && !path.is_empty() {
            root_only(table, &path, &found.at)?;
        }
        nodes.push(Node {
            path: path.clone(),
            kind: found.kind.clone(),
            at: found.at.clone(),
        });
    }

    ancestors.push(here);
    for pair in table.pairs::<mlua::Value, mlua::Value>() {
        let (key, value) = pair?;
        let mlua::Value::Table(child) = value else {
            continue;
        };
        let segment = match &key {
            mlua::Value::String(key) => key.to_str()?.to_string(),
            mlua::Value::Integer(index) => format!("[{index}]"),
            _ => continue,
        };
        let child_path = match (path.is_empty(), segment.starts_with('[')) {
            (true, _) | (false, true) => format!("{path}{segment}"),
            (false, false) => format!("{path}.{segment}"),
        };

        if let (Some(parent), mlua::Value::String(key)) = (&found, &key) {
            match expected(&parent.kind, &key.to_str()?) {
                Some(Expect::One(kind)) => check(&child, kind, &child_path)?,
                Some(Expect::MapOf(kind)) => {
                    for entry in child.pairs::<mlua::Value, mlua::Value>() {
                        let (name, value) = entry?;
                        let entry_path = match name {
                            mlua::Value::String(name) => format!("{child_path}.{}", name.to_str()?),
                            other => format!("{child_path}[{other:?}]"),
                        };
                        match value {
                            mlua::Value::Table(entry) => check(&entry, kind, &entry_path)?,
                            other => {
                                return Err(Error::NotATable {
                                    what: entry_path,
                                    expected: kind,
                                    got: other.type_name(),
                                })
                            }
                        }
                    }
                }
                Some(Expect::Fields(fields)) => {
                    for (name, kind) in fields {
                        let entry: mlua::Value = child.raw_get(*name)?;
                        let entry_path = format!("{child_path}.{name}");
                        match entry {
                            mlua::Value::Nil => {}
                            mlua::Value::Table(entry) => check(&entry, kind, &entry_path)?,
                            other => {
                                return Err(Error::NotATable {
                                    what: entry_path,
                                    expected: kind,
                                    got: other.type_name(),
                                })
                            }
                        }
                    }
                }
                None => {}
            }
        }

        visit(&child, child_path, ancestors, nodes)?;
    }
    ancestors.pop();
    Ok(())
}
