//! A line diff for the overwrite prompt: what a user's copy of a built-in
//! holds against the new built-in. The files are a few hundred lines, so
//! the plain longest-common-subsequence table is fast enough.

/// One line of the comparison.
#[derive(Debug, PartialEq)]
enum Line<'a> {
    Same(&'a str),
    /// only in the user's copy
    Yours(&'a str),
    /// only in the new built-in
    New(&'a str),
}

fn compare<'a>(yours: &'a str, new: &'a str) -> Vec<Line<'a>> {
    let a: Vec<&str> = yours.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    // common[i][j]: the longest run both share from a[i..] and b[j..]
    let mut common = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            common[i][j] = if a[i] == b[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }

    let (mut i, mut j) = (0, 0);
    let mut lines = Vec::with_capacity(a.len().max(b.len()));
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            lines.push(Line::Same(a[i]));
            i += 1;
            j += 1;
        } else if common[i + 1][j] >= common[i][j + 1] {
            lines.push(Line::Yours(a[i]));
            i += 1;
        } else {
            lines.push(Line::New(b[j]));
            j += 1;
        }
    }
    lines.extend(a[i..].iter().map(|line| Line::Yours(line)));
    lines.extend(b[j..].iter().map(|line| Line::New(line)));
    lines
}

/// The changes between `yours` and `new`, each with `context` unchanged
/// lines around it: `-` for a line only in yours, `+` for one only in the
/// new built-in, and `@@ line N @@` where a group starts in yours.
/// Empty when the two are the same.
pub(crate) fn diff(yours: &str, new: &str, context: usize) -> String {
    let lines = compare(yours, new);
    let changed: Vec<bool> = lines
        .iter()
        .map(|line| !matches!(line, Line::Same(_)))
        .collect();
    let near = |at: usize| {
        let from = at.saturating_sub(context);
        let to = (at + context + 1).min(lines.len());
        changed[from..to].iter().any(|&c| c)
    };

    let mut out = String::new();
    let mut in_group = false;
    let mut yours_line = 0;
    for (at, line) in lines.iter().enumerate() {
        if !near(at) {
            in_group = false;
            if !matches!(line, Line::New(_)) {
                yours_line += 1;
            }
            continue;
        }
        if !in_group {
            out.push_str(&format!("@@ line {} @@\n", yours_line + 1));
            in_group = true;
        }
        match line {
            Line::Same(text) => {
                out.push_str(&format!("  {text}\n"));
                yours_line += 1;
            }
            Line::Yours(text) => {
                out.push_str(&format!("- {text}\n"));
                yours_line += 1;
            }
            Line::New(text) => out.push_str(&format!("+ {text}\n")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_has_no_diff() {
        assert_eq!(diff("a\nb\n", "a\nb\n", 2), "");
    }

    #[test]
    fn a_changed_line_shows_both_sides_with_context() {
        let yours = "one\ntwo\nthree\nfour\nfive\nsix\nseven\n";
        let new = "one\ntwo\nthree\nFOUR\nfive\nsix\nseven\n";
        assert_eq!(
            diff(yours, new, 1),
            "@@ line 3 @@\n  three\n- four\n+ FOUR\n  five\n"
        );
    }

    #[test]
    fn changes_far_apart_are_separate_groups() {
        let yours = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "A\nb\nc\nd\ne\nf\ng\nH\n";
        let out = diff(yours, new, 1);
        assert_eq!(out.matches("@@").count(), 4, "two groups: {out}");
        assert!(out.starts_with("@@ line 1 @@\n- a\n+ A\n  b\n"), "{out}");
        assert!(out.ends_with("@@ line 7 @@\n  g\n- h\n+ H\n"), "{out}");
    }

    #[test]
    fn added_and_removed_lines() {
        assert_eq!(diff("a\nc\n", "a\nb\nc\n", 0), "@@ line 2 @@\n+ b\n");
        assert_eq!(diff("a\nb\nc\n", "a\nc\n", 0), "@@ line 2 @@\n- b\n");
    }
}
