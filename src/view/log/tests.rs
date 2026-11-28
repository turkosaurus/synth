use super::{line, WIDTH};

/// Wide enough that no test event wraps.
const WIDE: usize = 200;

const EVENT: &str = r#"{"timestamp":"2026-11-27T10:11:12.345678Z","level":"WARN","fields":{"message":"underrun","frames":512,"device":"hw:0"},"target":"synth::audio::fault"}"#;

#[test]
fn an_event_reads_as_time_level_message_then_fields() {
    assert_eq!(
        line(EVENT, false, WIDE),
        "10:11:12.345  WARN underrun frames=512 device=hw:0"
    );
}

#[test]
fn long_fields_go_one_per_line() {
    let narrow = 30;
    assert_eq!(
        line(EVENT, false, narrow),
        "10:11:12.345  WARN underrun\n\
         \x20                  frames=512\n\
         \x20                  device=hw:0"
    );
}

#[test]
fn colour_adds_escapes_around_the_same_text() {
    let coloured = line(EVENT, true, WIDTH);
    if std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) {
        assert!(coloured.contains('\x1b'), "{coloured:?}");
    }
    assert!(coloured.contains("underrun"));
}

#[test]
fn a_span_close_reads_as_its_path_and_times() {
    const CLOSE: &str = r#"{"timestamp":"2026-11-27T21:34:37.832011Z","level":"DEBUG","fields":{"message":"close","time.busy":"1.54ms","time.idle":"27.9µs"},"target":"synth::script::bake","span":{"points":2048,"name":"bake_wave"},"spans":[{"path":"\"lua/example.lua\"","name":"load"},{"sample_rate":48000,"name":"bake_instrument"}]}"#;
    assert_eq!(
        line(CLOSE, false, WIDE),
        "21:34:37.832 DEBUG load > bake_instrument > bake_wave busy=1.54ms idle=27.9µs"
    );
}

#[test]
fn a_line_that_is_not_an_event_passes_through() {
    assert_eq!(line("plain text", false, WIDTH), "plain text");
    assert_eq!(line("[1, 2]", false, WIDTH), "[1, 2]");
}
