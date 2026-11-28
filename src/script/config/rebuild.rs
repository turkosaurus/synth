//! A user's `config.lua` written back over the template: each value the
//! user set as a Lua literal in place of the default, the template's
//! comments and order kept, the user's own lost.

use std::collections::BTreeMap;

use super::Error;

/// What a user's file sets: literals, and tables of them.
#[derive(Debug, PartialEq)]
pub(super) enum Set {
    Literal(String),
    Table(BTreeMap<String, Set>),
}

/// Everything `table` sets, as literals. `name` names the file in errors.
pub(super) fn values(table: &mlua::Table, name: &str) -> Result<BTreeMap<String, Set>, Error> {
    let mut set = BTreeMap::new();
    for pair in table.pairs::<mlua::Value, mlua::Value>() {
        let (key, value) = pair?;
        // the schema ignores array entries, and so does loading
        let Some(key) = key.as_string().and_then(|s| s.to_str().ok()) else {
            continue;
        };
        let value = match value {
            mlua::Value::Table(inner) => Set::Table(values(&inner, name)?),
            other => Set::Literal(literal(&other).ok_or_else(|| Error::ConfigRebuild {
                name: name.to_string(),
            })?),
        };
        set.insert(key.to_string(), value);
    }
    Ok(set)
}

/// `value` as Lua source; `None` for one with no literal form.
fn literal(value: &mlua::Value) -> Option<String> {
    Some(match value {
        mlua::Value::Boolean(flag) => flag.to_string(),
        mlua::Value::Integer(n) => n.to_string(),
        mlua::Value::Number(n) if n.is_nan() => "0/0".to_string(),
        mlua::Value::Number(n) if n.is_infinite() => {
            if *n > 0.0 { "1/0" } else { "-1/0" }.to_string()
        }
        // Rust's shortest form reads back to the same double
        mlua::Value::Number(n) => n.to_string(),
        mlua::Value::String(text) => quoted(&text.as_bytes()),
        _ => return None,
    })
}

/// `bytes` as a double-quoted Lua string. Escapes are decimal and three
/// digits wide, which Lua 5.1 reads and a following digit cannot extend.
fn quoted(bytes: &[u8]) -> String {
    let utf8 = std::str::from_utf8(bytes).is_ok();
    let mut out = vec![b'"'];
    for &byte in bytes {
        match byte {
            b'"' | b'\\' => out.extend([b'\\', byte]),
            b'\n' => out.extend(*b"\\n"),
            0x20..=0x7e => out.push(byte),
            0x80.. if utf8 => out.push(byte),
            _ => out.extend(format!("\\{byte:03}").bytes()),
        }
    }
    out.push(b'"');
    String::from_utf8_lossy(&out).into_owned()
}

/// `set` as a one-line table, in key order.
fn inline(set: BTreeMap<String, Set>) -> String {
    let fields: Vec<String> = set
        .into_iter()
        .map(|(key, value)| format!("{} = {}", name(&key), written(value)))
        .collect();
    format!("{{ {} }}", fields.join(", "))
}

fn written(value: Set) -> String {
    match value {
        Set::Literal(literal) => literal,
        Set::Table(set) => inline(set),
    }
}

/// `key` as a table field's name: bare when it is an identifier.
fn name(key: &str) -> String {
    let mut chars = key.chars();
    let bare = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    match bare {
        true => key.to_string(),
        false => format!("[{}]", quoted(key.as_bytes())),
    }
}

/// `default`, a one-line table from the template such as
/// `{ warn = 1, danger = 5 }`, with `set` laid over it. Fields keep the
/// template's order; any it lacks follow.
fn over_inline(default: &str, mut set: BTreeMap<String, Set>) -> String {
    let inner = default
        .trim()
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or("");
    let mut fields = Vec::new();
    for field in inner.split(',').map(str::trim).filter(|f| !f.is_empty()) {
        let Some((key, value)) = field.split_once(" = ") else {
            continue;
        };
        let value = set.remove(key).map_or(value.to_string(), written);
        fields.push(format!("{key} = {value}"));
    }
    fields.extend(
        set.into_iter()
            .map(|(key, value)| format!("{} = {}", name(&key), written(value))),
    );
    format!("{{ {} }}", fields.join(", "))
}

/// A template line that names a field: its indent, whether it is
/// commented out, its key, and what follows ` = `.
struct Field<'a> {
    indent: &'a str,
    commented: bool,
    key: &'a str,
    rest: &'a str,
}

fn field(line: &str) -> Option<Field<'_>> {
    let body = line.trim_start();
    let indent = &line[..line.len() - body.len()];
    let (commented, body) = match body.strip_prefix("-- ") {
        Some(body) => (true, body),
        None => (false, body),
    };
    let (key, rest) = body.split_once(" = ")?;
    (name(key) == key).then_some(Field {
        indent,
        commented,
        key,
        rest,
    })
}

/// `template` with `set` written in place of its defaults. A commented-out
/// field the user set is written uncommented; a field the template lacks
/// is written at the end of its table.
pub(super) fn merge(template: &str, set: BTreeMap<String, Set>) -> String {
    let mut out = String::new();
    // each open table's contents indent and what of it is not yet written
    let mut open: Vec<(String, BTreeMap<String, Set>)> = Vec::new();
    let mut root = Some(set);
    for line in template.lines() {
        let trimmed = line.trim();
        let Some((_, here)) = open.last_mut() else {
            // `local config = {`
            if trimmed.ends_with("= {") && !trimmed.starts_with("--") {
                if let Some(set) = root.take() {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    open.push((format!("{indent}\t"), set));
                }
            }
            out.push_str(line);
            out.push('\n');
            continue;
        };
        if trimmed == "}" || trimmed == "}," {
            if let Some((indent, rest)) = open.pop() {
                for (key, value) in rest {
                    out.push_str(&format!("{indent}{} = {},\n", name(&key), written(value)));
                }
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let Some(field) = field(line) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        if field.rest == "{" && !field.commented {
            let inner = match here.remove(field.key) {
                Some(Set::Table(inner)) => inner,
                Some(literal) => {
                    // not a table here; written at the table's end instead
                    here.insert(field.key.to_string(), literal);
                    BTreeMap::new()
                }
                None => BTreeMap::new(),
            };
            open.push((format!("{}\t", field.indent), inner));
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let Some(value) = here.remove(field.key) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let comment = field.rest.find(" --").map_or("", |at| &field.rest[at..]);
        let default = field.rest[..field.rest.len() - comment.len()].trim_end_matches(',');
        let value = match value {
            Set::Literal(literal) => literal,
            Set::Table(set) if default.starts_with('{') => over_inline(default, set),
            Set::Table(set) => inline(set),
        };
        out.push_str(&format!(
            "{}{} = {value},{comment}\n",
            field.indent, field.key
        ));
    }
    out
}
