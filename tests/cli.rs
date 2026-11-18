use std::process::Command;

/// `--version` prints one line a script can split on the space.
#[test]
fn version_flag() {
    let out = Command::new(env!("CARGO_BIN_EXE_synth"))
        .arg("--version")
        .output()
        .expect("run synth --version");
    assert!(out.status.success());
    let expected = format!("synth {}\n", synth::release::VERSION);
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}
