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

/// `install` copies the running binary to `XDG_BIN_HOME`,
/// and the installed copy's `uninstall` removes it.
#[cfg(unix)]
#[test]
fn install_then_uninstall() {
    let root = std::env::temp_dir().join(format!("synth-cli-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (src, bin) = (root.join("src"), root.join("bin"));
    std::fs::create_dir_all(&src).unwrap();
    let exe = src.join("synth");
    std::fs::copy(env!("CARGO_BIN_EXE_synth"), &exe).unwrap();

    let run = |exe: &std::path::Path, arg: &str| {
        let out = Command::new(exe)
            .arg(arg)
            .env("XDG_BIN_HOME", &bin)
            .output()
            .expect("run synth");
        assert!(
            out.status.success(),
            "{arg}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let installed = bin.join("synth");
    assert!(run(&exe, "install").starts_with("installed synth"));
    assert!(installed.exists());
    assert!(run(&installed, "install").starts_with("synth is installed at"));
    assert!(run(&installed, "uninstall").starts_with("removed"));
    assert!(!installed.exists());
    assert!(exe.exists(), "uninstall removes only the binary it runs as");

    std::fs::remove_dir_all(&root).unwrap();
}
