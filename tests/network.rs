//! Tests that reach GitHub releases. `cargo test` skips them;
//! `mise run test:network` runs them, and CI runs both suites.

use std::process::Command;

/// `update --dry-run` end to end against the live release page: HTTPS, the
/// release list, the sha256 digest, and the unpack, replacing nothing.
#[test]
#[ignore = "network: GitHub releases"]
fn update_dry_run() {
    let out = Command::new(env!("CARGO_BIN_EXE_synth"))
        .args(["update", "--dry-run"])
        .output()
        .expect("run synth update --dry-run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains(" verified; would replace "), "{stdout}");
}
