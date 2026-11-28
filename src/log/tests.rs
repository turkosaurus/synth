use super::*;
use std::io::Write;

fn scratch(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("synth-log-{test}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir.join("log.jsonl")
}

#[test]
fn a_session_appends() {
    let path = scratch("appends");
    fs::write(&path, "one\n").unwrap();
    writeln!(open(&path, 0).unwrap(), "two").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_large_log_keeps_its_newest_whole_lines() {
    let path = scratch("trims");
    fs::write(&path, "first\nsecond\nthird\n").unwrap();
    // 9 bytes reach back into "second", so that line goes too
    open(&path, 9).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "third\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_cut_on_a_line_boundary_keeps_that_line() {
    let path = scratch("boundary");
    fs::write(&path, "first\nsecond\n").unwrap();
    open(&path, 7).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_log_within_the_limit_or_unlimited_is_untouched() {
    let path = scratch("untouched");
    fs::write(&path, "first\nsecond\n").unwrap();
    open(&path, 13).unwrap();
    open(&path, 0).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_missing_log_is_created() {
    let path = scratch("missing");
    let _ = fs::remove_file(&path);
    open(&path, 9).unwrap();
    assert!(path.exists());
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
