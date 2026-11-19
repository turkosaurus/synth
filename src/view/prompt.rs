//! The question `synth patches` asks about an edited copy of a built-in
//! that has a newer default. Reads and writes through any `BufRead` and
//! `Write`, so a test can answer it.

use std::io::{self, BufRead, Write};
use std::path::Path;

use super::diff::diff;

/// Lines of unchanged text around each difference `d` shows.
const CONTEXT: usize = 2;

/// What the user chose for one copy.
#[derive(Debug, PartialEq)]
pub enum Choice {
    /// replace the copy with the new built-in
    Overwrite,
    /// keep the copy as it is
    Keep,
}

/// Asks whether to overwrite `path`, the user's `copy`, with `new`. `d`
/// shows the difference, colored when `color` is set, and asks again;
/// anything else unrecognised asks again. The end of input keeps the copy,
/// so nothing is lost when no one is there to answer.
pub fn overwrite(
    path: &Path,
    copy: &str,
    new: &str,
    color: bool,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> io::Result<Choice> {
    loop {
        write!(
            output,
            "update would overwrite {} with new default (y/n/d) ",
            path.display()
        )?;
        output.flush()?;
        let mut answer = String::new();
        if input.read_line(&mut answer)? == 0 {
            writeln!(output)?;
            return Ok(Choice::Keep);
        }
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(Choice::Overwrite),
            "n" | "no" => return Ok(Choice::Keep),
            "d" | "diff" => {
                writeln!(output, "--- yours\n+++ new default")?;
                write!(output, "{}", diff(copy, new, CONTEXT, color))?;
            }
            _ => writeln!(
                output,
                "answer y to overwrite, n to keep yours, or d to see the difference"
            )?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(answers: &str) -> (Choice, String) {
        let mut output = Vec::new();
        let choice = overwrite(
            Path::new("patches/synth/envelope.lua"),
            "attack = 5\n",
            "attack = 15\n",
            false,
            &mut answers.as_bytes(),
            &mut output,
        )
        .unwrap();
        (choice, String::from_utf8(output).unwrap())
    }

    #[test]
    fn y_overwrites_and_n_keeps() {
        assert_eq!(ask("y\n").0, Choice::Overwrite);
        assert_eq!(ask("YES\n").0, Choice::Overwrite);
        assert_eq!(ask("n\n").0, Choice::Keep);
    }

    #[test]
    fn d_shows_the_difference_then_asks_again() {
        let (choice, output) = ask("d\ny\n");
        assert_eq!(choice, Choice::Overwrite);
        assert!(output.contains("- attack = 5\n+ attack = 15\n"), "{output}");
        assert_eq!(output.matches("(y/n/d)").count(), 2, "{output}");
    }

    #[test]
    fn an_unknown_answer_asks_again() {
        let (choice, output) = ask("maybe\nn\n");
        assert_eq!(choice, Choice::Keep);
        assert!(output.contains("answer y to overwrite"), "{output}");
    }

    #[test]
    fn no_answer_keeps_the_copy() {
        assert_eq!(ask("").0, Choice::Keep);
    }
}
