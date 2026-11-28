//! The question `synth custom` asks about an edited copy of a built-in
//! that has a newer default, and the one `synth config` asks before
//! rebuilding an out-of-date file. Reads and writes through any `BufRead` and
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

/// What `synth config` asks before rebuilding an out-of-date file.
pub const REBUILD: &str =
    "Prior values will be kept, formatting and comments will be lost. Continue...";

/// Asks `question` until answered y or n. The end of input is no.
pub fn confirm(
    question: &str,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> io::Result<bool> {
    loop {
        write!(output, "{question} (y/n) ")?;
        output.flush()?;
        let mut answer = String::new();
        if input.read_line(&mut answer)? == 0 {
            writeln!(output)?;
            return Ok(false);
        }
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(output, "answer y or n")?,
        }
    }
}

#[cfg(test)]
mod tests;
