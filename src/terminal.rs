//! Raw mode and key release detection.

use crossterm::{
    event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    execute, terminal,
};
use std::io::{self, IsTerminal};
use std::panic;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Once, OnceLock};

use crate::input::keyboard::Gating;

/// Set by the handler for a termination signal. The handler only stores:
/// writing escape codes is not async-signal-safe, so the view's poll loop
/// reads this and leaves through [`leave`] like any other exit.
static INTERRUPTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Set by the panic hook once it has restored the terminal. A panic on
/// another thread, such as the audio callback's, leaves the view running,
/// and it must stop drawing over the screen the message was printed to.
static PANICKED: AtomicBool = AtomicBool::new(false);

/// What [`enter`] has done to the terminal so far, one bit per step. The
/// panic hook does not know the gating mode, and a step that failed must not
/// be undone, so [`leave`] reads this rather than being told.
static ENTERED: AtomicU8 = AtomicU8::new(0);
const RAW: u8 = 1;
const ALTERNATE: u8 = 1 << 1;
const ENHANCED: u8 = 1 << 2;

/// An error unless stdin and stdout are both terminals. Without one there
/// is no key to wait for, and the view would redraw into nothing forever.
pub fn require() -> io::Result<()> {
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return Ok(());
    }
    Err(io::Error::other(
        "synth tui needs a terminal on stdin and stdout",
    ))
}

/// Whether a termination signal has arrived, or a panic has restored the
/// terminal under the view. False on a platform with no signal handler
/// registered, which leaves the old behaviour untouched.
pub(crate) fn interrupted() -> bool {
    PANICKED.load(Ordering::Relaxed)
        || INTERRUPTED
            .get()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// Restores the terminal when dropped, so a normal return, an error passed
/// up with `?` and an unwinding panic all leave through [`leave`].
///
/// A panic is covered by the hook from [`watch_panics`] under either panic
/// strategy; with `panic = "abort"` the hook is the only thing that runs.
/// Under unwinding this guard drops after the hook and finds nothing left
/// to undo. Errors and normal returns are this guard's alone.
#[must_use = "the terminal is restored when the guard drops"]
pub(crate) struct Guard(());

impl Drop for Guard {
    fn drop(&mut self) {
        leave();
    }
}

/// Put the terminal into the state an interactive view needs. The returned
/// guard puts it back when dropped.
pub(crate) fn enter(gating: Gating) -> io::Result<Guard> {
    watch_signals()?;
    watch_panics();
    // made before the first step, so a step that fails drops it and undoes
    // the steps before it
    let guard = Guard(());
    terminal::enable_raw_mode()?;
    ENTERED.fetch_or(RAW, Ordering::SeqCst);
    execute!(io::stdout(), terminal::EnterAlternateScreen)?;
    ENTERED.fetch_or(ALTERNATE, Ordering::SeqCst);
    enable_gating(gating)?;
    Ok(guard)
}

/// Restore the terminal before the previous hook prints the message, so it
/// lands on the normal screen in cooked mode where it can be read.
///
/// The hook runs on whichever thread panics, the audio callback's included.
/// That is safe: [`leave`] takes the steps to undo with one atomic swap, so
/// the hook and the guard never both undo a step, and stdout is behind std's
/// lock. Restoring from that thread is acceptable because the sound has
/// already stopped, and the view sees [`PANICKED`] and quits within a frame
/// instead of drawing over the message.
fn watch_panics() {
    // one hook per process. a second would chain to the first and run the
    // restore twice for every panic.
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if leave() {
                PANICKED.store(true, Ordering::Relaxed);
            }
            previous(info);
        }));
    });
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

/// Reverse of [`enter`], in reverse order, and only the steps it finished.
/// Reports whether there was anything to undo. A second call finds nothing:
/// popping the keyboard flags twice would pop an entry the shell pushed.
/// Errors are discarded: on the way out there is nothing useful to do about
/// them.
fn leave() -> bool {
    let entered = ENTERED.swap(0, Ordering::SeqCst);
    if entered & ENHANCED != 0 {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    if entered & ALTERNATE != 0 {
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen);
    }
    if entered & RAW != 0 {
        let _ = terminal::disable_raw_mode();
    }
    entered != 0
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
        ENTERED.fetch_or(ENHANCED, Ordering::SeqCst);
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

#[cfg(test)]
mod tests;
