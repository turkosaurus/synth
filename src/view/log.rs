//! One JSON log event as a line to read.

use crossterm::style::{Color, Stylize};
use serde_json::Value;

/// Where fields wrap when the terminal's width is unknown.
pub const WIDTH: usize = 80;

/// The terminal's width, or [`WIDTH`] without one.
pub fn width() -> usize {
    crossterm::terminal::size().map_or(WIDTH, |(cols, _)| usize::from(cols))
}

/// `event`, a line of the JSON log, as time, level, message, then
/// `key=value` fields; the fields go one per line, indented, when the
/// event would run past `width`. With `color`, the level is coloured and
/// time and keys are dimmed. A span's close reads as its path, outermost
/// first, with `busy` and `idle` times. A line that is not an event passes
/// through.
pub fn line(event: &str, color: bool, width: usize) -> String {
    let Ok(Value::Object(event)) = serde_json::from_str::<Value>(event) else {
        return event.to_string();
    };
    let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or("");
    // RFC 3339; the date is the file's, and a session rarely spans one
    let time = text("timestamp");
    let time = time.split_once('T').map_or(time, |(_, time)| time);
    let time = time.get(..12).unwrap_or(time);
    let level = text("level");
    let empty = serde_json::Map::new();
    let fields = event
        .get("fields")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut message = fields.get("message").map(value).unwrap_or_default();
    let closed = span_path(&event);
    if let (Some(path), "close") = (&closed, message.as_str()) {
        message = path.clone();
    }
    let pairs: Vec<(&str, String)> = fields
        .iter()
        .filter(|(key, _)| *key != "message")
        .map(|(key, field)| {
            let key = match closed {
                Some(_) => key.strip_prefix("time.").unwrap_or(key),
                None => key,
            };
            (key, value(field))
        })
        .collect();

    let head = format!("{time} {level:>5} {message}");
    let flat: usize = pairs.iter().map(|(k, v)| k.len() + v.len() + 2).sum();
    let wrap = head.chars().count() + flat > width;

    let paint = |text: &str, as_color: Color| match color {
        true => text.with(as_color).to_string(),
        false => text.to_string(),
    };
    let mut out = format!(
        "{} {} {message}",
        paint(time, Color::DarkGrey),
        paint(&format!("{level:>5}"), level_color(level)),
    );
    let indent = " ".repeat(time.chars().count() + 7);
    for (key, field) in pairs {
        out.push_str(if wrap { "\n" } else { " " });
        if wrap {
            out.push_str(&indent);
        }
        out.push_str(&format!(
            "{}{field}",
            paint(&format!("{key}="), Color::DarkGrey)
        ));
    }
    out
}

/// `outer > inner` for the span an event is in, its parents first.
fn span_path(event: &serde_json::Map<String, Value>) -> Option<String> {
    let name = |span: &Value| span.get("name").and_then(Value::as_str).map(str::to_string);
    let current = name(event.get("span")?)?;
    let mut path: Vec<String> = event
        .get("spans")
        .and_then(Value::as_array)
        .map(|spans| spans.iter().filter_map(name).collect())
        .unwrap_or_default();
    // some formatters list the current span among its parents
    if path.last() != Some(&current) {
        path.push(current);
    }
    Some(path.join(" > "))
}

fn level_color(level: &str) -> Color {
    match level {
        "ERROR" => Color::Red,
        "WARN" => Color::Yellow,
        "INFO" => Color::Green,
        "DEBUG" => Color::Blue,
        _ => Color::Magenta,
    }
}

/// A string as it is, anything else as JSON.
fn value(field: &Value) -> String {
    match field {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
