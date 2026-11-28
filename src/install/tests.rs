use super::*;

/// `path`, a Unix-style absolute path, made absolute on the test host:
/// Windows needs a drive, so `/h` becomes `C:/h` there.
fn abs(path: &str) -> PathBuf {
    match cfg!(windows) {
        true => PathBuf::from(format!("C:{path}")),
        false => PathBuf::from(path),
    }
}

/// An environment value: absolute ones go through [`abs`], relative
/// ones are passed as written.
fn var(value: &str) -> OsString {
    match value.starts_with('/') {
        true => abs(value).into_os_string(),
        false => OsString::from(value),
    }
}

fn dir(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    bin_dir(xdg.map(var), home.map(var), None, false)
}

#[test]
fn windows_falls_back_to_userprofile() {
    let profile = Some(var("/users/me"));
    assert_eq!(
        bin_dir(None, None, profile.clone(), true),
        Some(abs("/users/me/.local/bin"))
    );
    assert_eq!(
        bin_dir(None, Some(var("/h")), profile.clone(), true),
        Some(abs("/h/.local/bin")),
        "HOME still wins when set"
    );
    assert_eq!(bin_dir(None, None, profile, false), None, "Unix ignores it");
}

#[test]
fn place_replaces_an_existing_binary() {
    let dir = std::env::temp_dir().join(format!("synth-place-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let to = dir.join(FILE);
    fs::write(&to, b"old").unwrap();
    place(b"new", &to).unwrap();
    assert_eq!(fs::read(&to).unwrap(), b"new");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bin_dir_prefers_xdg() {
    assert_eq!(dir(Some("/x/bin"), Some("/h")), Some(abs("/x/bin")));
    assert_eq!(dir(None, Some("/h")), Some(abs("/h/.local/bin")));
}

#[test]
fn bin_dir_ignores_relative_paths() {
    assert_eq!(dir(Some("bin"), Some("/h")), Some(abs("/h/.local/bin")));
    assert_eq!(dir(Some(""), Some("/h")), Some(abs("/h/.local/bin")));
    assert_eq!(dir(None, Some("h")), None);
    assert_eq!(dir(None, None), None);
}
