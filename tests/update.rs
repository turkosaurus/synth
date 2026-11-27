//! `update` against the live GitHub releases. `cargo test` skips them;
//! `mise run test:update` runs them, and `mise run tests` runs everything.

use std::fs;
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

/// `update --dev` from an installed copy with a custom folder written:
/// the copy is replaced while it runs, and the new binary syncs its
/// built-in copies.
#[test]
#[ignore = "network: GitHub releases"]
fn update_replaces_itself() {
    let dir = std::env::temp_dir().join(format!("synth-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).expect("create bin");
    let exe = bin.join(format!("synth{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_synth"), &exe).expect("copy synth");
    let synth = |args: &[&str]| {
        Command::new(&exe)
            .args(args)
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_DATA_HOME", dir.join("data"))
            .output()
            .expect("run synth")
    };

    let wrote = synth(&["custom", "--sync"]);
    assert!(
        wrote.status.success(),
        "{}",
        String::from_utf8_lossy(&wrote.stderr)
    );

    // a dev build's version carries +sha, so it is never this build's
    let out = synth(&["update", "--dev"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // a build from before the rename has no `custom`; its sync fails with a
    // warning, and the update still succeeds
    let knows_custom = synth(&["custom", "--path"]).status.success();
    let _ = fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{stderr}");
    assert!(stdout.contains(" -> "), "{stdout}");
    if knows_custom {
        assert!(!stderr.contains("custom --sync"), "{stderr}");
    } else {
        assert!(stderr.contains("custom --sync exited with"), "{stderr}");
    }
}
