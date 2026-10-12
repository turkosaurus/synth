use crossterm::{
    event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    execute, terminal,
};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::input::keyboard::Gating;

/// Set by the handler for a termination signal. The handler only stores:
/// writing escape codes is not async-signal-safe, so the view's poll loop
/// reads this and leaves through [`leave`] like any other exit.
static INTERRUPTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Whether a termination signal has arrived. False on a platform with no
/// handler registered, which leaves the old behaviour untouched.
pub fn interrupted() -> bool {
    INTERRUPTED
        .get()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// Put the terminal into the state an interactive view needs, and report
/// whether gating ended up Held.
pub fn enter(gating: Gating) -> io::Result<()> {
    watch_signals()?;
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), terminal::EnterAlternateScreen)?;
    enable_gating(gating)
}

// SIGINT is absent on purpose: raw mode disables ISIG, so ctrl-c arrives as
// a key event and the keyboard already answers it with Outcome::Quit.
#[cfg(unix)]
fn watch_signals() -> io::Result<()> {
    use signal_hook::consts::{SIGHUP, SIGTERM};

    // one registration per process. a second would install a handler
    // writing to a flag that nothing reads.
    if INTERRUPTED.get().is_some() {
        return Ok(());
    }
    let flag = Arc::new(AtomicBool::new(false));
    for signal in [SIGHUP, SIGTERM] {
        signal_hook::flag::register(signal, Arc::clone(&flag))?;
    }
    let _ = INTERRUPTED.set(flag);
    Ok(())
}

#[cfg(not(unix))]
fn watch_signals() -> io::Result<()> {
    Ok(())
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
    matches!(crossterm::terminal::supports_keyboard_enhancement(), Ok(true))
}
