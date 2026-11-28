use super::*;

#[test]
fn the_same_text_has_no_diff() {
    assert_eq!(diff("a\nb\n", "a\nb\n", 2, false), "");
}

#[test]
fn a_changed_line_shows_both_sides_with_context() {
    let yours = "one\ntwo\nthree\nfour\nfive\nsix\nseven\n";
    let new = "one\ntwo\nthree\nFOUR\nfive\nsix\nseven\n";
    assert_eq!(
        diff(yours, new, 1, false),
        "@@ line 3 @@\n  three\n- four\n+ FOUR\n  five\n"
    );
}

#[test]
fn changes_far_apart_are_separate_groups() {
    let yours = "a\nb\nc\nd\ne\nf\ng\nh\n";
    let new = "A\nb\nc\nd\ne\nf\ng\nH\n";
    let out = diff(yours, new, 1, false);
    assert_eq!(out.matches("@@").count(), 4, "two groups: {out}");
    assert!(out.starts_with("@@ line 1 @@\n- a\n+ A\n  b\n"), "{out}");
    assert!(out.ends_with("@@ line 7 @@\n  g\n- h\n+ H\n"), "{out}");
}

#[test]
fn color_marks_each_kind_of_line() {
    let out = diff("a\n", "b\n", 0, true);
    // red, green and cyan, unless NO_COLOR is set where the test runs
    if std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) {
        assert!(out.contains("\u{1b}[38;5;9m- a"), "{out:?}");
        assert!(out.contains("\u{1b}[38;5;10m+ b"), "{out:?}");
        assert!(out.contains("\u{1b}[38;5;14m@@"), "{out:?}");
    }
    assert_eq!(diff("a\n", "b\n", 0, false), "@@ line 1 @@\n- a\n+ b\n");
}

#[test]
fn added_and_removed_lines() {
    assert_eq!(diff("a\nc\n", "a\nb\nc\n", 0, false), "@@ line 2 @@\n+ b\n");
    assert_eq!(diff("a\nb\nc\n", "a\nc\n", 0, false), "@@ line 2 @@\n- b\n");
}
