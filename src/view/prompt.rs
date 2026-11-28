//! The question `synth custom` asks about an edited copy of a built-in
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
mod tests;
