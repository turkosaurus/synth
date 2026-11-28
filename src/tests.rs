use super::{editor, Args};
use clap::Parser;

#[test]
fn doctor_takes_no_file() {
    assert!(Args::try_parse_from(["synth", "doctor"]).is_ok());
    assert!(Args::try_parse_from(["synth", "doctor", "patch.lua"]).is_err());
}

fn some(value: &str) -> Option<String> {
    Some(value.to_string())
}

#[test]
fn visual_wins_then_editor_then_vi() {
    assert_eq!(editor(some("nvim"), some("nano")), ["nvim"]);
    assert_eq!(editor(None, some("nano")), ["nano"]);
    assert_eq!(editor(some(" "), some("nano")), ["nano"]);
    assert_eq!(editor(None, None), ["vi"]);
}

#[test]
fn an_editor_keeps_its_flags() {
    assert_eq!(editor(some("code --wait"), None), ["code", "--wait"]);
}
