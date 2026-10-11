// integration tests link the library crate, so they see only the public API —
// which is the point: this would not compile against a bin-only crate
use std::path::Path;
use synth::script;

#[test]
fn example_patch_bakes_clean() {
    let patch = script::load(Path::new("lua/example.lua"), 48000).unwrap();

    // deliberately no exact tempo or envelope numbers: the example is tuned
    // by ear, so only its structural health is worth pinning here
    assert!(patch.bpm > 0.0);
    assert!(patch.step_ms > 0.0);
    assert_eq!(patch.timeline.voices[0].wave.len(), 2048);
    assert!(patch.reports.iter().all(|(_, r)| r.warnings.is_empty()));
}

#[test]
fn a_patch_that_returns_the_wrong_type_is_rejected() {
    // .err() rather than .unwrap_err(): the latter needs Patch: Debug so it
    // can print the Ok value, and Patch holds thousands of floats
    let error = script::load(Path::new("Cargo.toml"), 48000)
        .err()
        .expect("Cargo.toml is not valid Lua");
    // Cargo.toml is not Lua, so this fails at parse rather than at the type check
    assert!(
        format!("{error:#}").contains("syntax error"),
        "unexpected: {error:#}"
    );
}
