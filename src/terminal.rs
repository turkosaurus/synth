use crossterm::{
    event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    execute, terminal,
};
use std::io;

use crate::input::keyboard::Gating;

/// Put the terminal into the state an interactive view needs, and report
/// whether gating ended up Held.
pub fn enter(gating: Gating) -> io::Result<()> {
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), terminal::EnterAlternateScreen)?;
    enable_gating(gating)
}

/// Reverse of [`enter`], in reverse order. Errors are discarded: on the way
/// out there is nothing useful to do about them.
pub fn leave(gating: Gating) {
    let _ = disable_gating(gating);
    let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

fn enable_gating(gating: Gating) -> io::Result<()> {
    if cfg!(windows) {
        return Ok(());
    }
    if let Gating::Held = gating {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(
                // DISAMBIGUATE so a lone Esc is not mistaken for the start of
                // an escape sequence; esc is the panic key and must be instant
                KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        )?;
    }
    Ok(())
}

fn disable_gating(gating: Gating) -> io::Result<()> {
    if cfg!(windows) {
        return Ok(());
    }
    if let Gating::Held = gating {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
    }
    Ok(())
}

// crossterm sets KeyEventKind on Windows unconditionally, but only on Unix
// once REPORT_EVENT_TYPES is pushed. Querying for the protocol there would
// answer "no" and cost us release events on a platform that already has them.
pub fn reports_key_release() -> bool {
    if cfg!(windows) {
        return true;
    }
    matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    )
}
