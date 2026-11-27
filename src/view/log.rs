//! One JSON log event as a line to read.

use crossterm::style::{Color, Stylize};
use serde_json::Value;

/// Where fields wrap when the terminal's width is unknown.
const WIDTH: usize = 100;

/// The terminal's width, or 100 columns without one.
pub fn width() -> usize {
    crossterm::terminal::size().map_or(WIDTH, |(cols, _)| usize::from(cols))
}

/// `event`, a line of the JSON log, as time, level, message, then
/// `key=value` fields; the fields go one per line, indented, when the
/// event would run past `width`. With `color`, the level is coloured and
/// time and keys are dimmed. A line that is not an event passes through.
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
    let message = fields.get("message").map(value).unwrap_or_default();
    let pairs: Vec<(&str, String)> = fields
        .iter()
        .filter(|(key, _)| *key != "message")
        .map(|(key, field)| (key.as_str(), value(field)))
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
mod tests {
    use super::line;

    const EVENT: &str = r#"{"timestamp":"2026-11-27T10:11:12.345678Z","level":"WARN","fields":{"message":"underrun","frames":512,"device":"hw:0"},"target":"synth::audio::fault"}"#;

    #[test]
    fn an_event_reads_as_time_level_message_then_fields() {
        assert_eq!(
            line(EVENT, false, 100),
            "10:11:12.345  WARN underrun frames=512 device=hw:0"
        );
    }

    #[test]
    fn long_fields_go_one_per_line() {
        assert_eq!(
            line(EVENT, false, 30),
            "10:11:12.345  WARN underrun\n\
             \x20                  frames=512\n\
             \x20                  device=hw:0"
        );
    }

    #[test]
    fn colour_adds_escapes_around_the_same_text() {
        let coloured = line(EVENT, true, 100);
        if std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) {
            assert!(coloured.contains('\x1b'), "{coloured:?}");
        }
        assert!(coloured.contains("underrun"));
    }

    #[test]
    fn a_line_that_is_not_an_event_passes_through() {
        assert_eq!(line("plain text", false, 100), "plain text");
        assert_eq!(line("[1, 2]", false, 100), "[1, 2]");
    }
}
