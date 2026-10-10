use crossterm::{
    cursor,
    event::{
        self, Event, KeyCode, KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal,
};
use std::io;
use std::time::Duration;


#[derive(Clone, Copy)]
pub enum Gating {
    Held,
    Fixed(Duration),
}

pub fn tui(gating: Gating) -> anyhow::Result<()> {
    // setup
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), terminal::EnterAlternateScreen)?;
    enable_gating(gating)?;

    // run + result
    let result = tui_loop();

    // teardown
    let _ = disable_gating(gating);
    let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    result
}

fn tui_loop() -> anyhow::Result<()> {
    let mut last = String::from("(none)");

    loop {
        execute!(
            io::stdout(),
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(0, 0)
        )?;
        // NOTE: raw mode stops translating \n, so \r\n is needed to return to column 0
        print!("last key: {last}\r\n");
        print!("esc or ctrl-c/d to quit\r\n");

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                // NOTE: contains(), not ==, so ctrl+shift+c still counts as ctrl
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char('c' | 'd') if ctrl => break,
                    // NOTE: key.kind is Press/Repeat/Release — Release only with gating held
                    _ => last = format!("{:?} / {:?}", key.code, key.kind),
                }
            }
        }
    }
    Ok(())
}

// there is no protocol to push or pop on Windows; release events arrive
// either way
fn enable_gating(gating: Gating) -> io::Result<()> {
    if cfg!(windows) {
        return Ok(());
    }
    if let Gating::Held = gating {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
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

pub fn doctor(gating: Gating) {
    let term = std::env::var("TERM").unwrap_or_default();
    let in_tmux = std::env::var("TMUX").is_ok();

    let (release, mode) = match gating {
        Gating::Held => ("yes".to_string(), "held notes".to_string()),
        Gating::Fixed(length) => ("no".to_string(), format!("fixed {} ms", length.as_millis())),
    };

    crate::ui::pairs(
        "environment",
        &[
            ("terminal", term),
            (
                "multiplexer",
                if in_tmux { "tmux" } else { "none" }.to_string(),
            ),
            ("key release", release),
            ("gating", mode),
        ],
        false,
    );

    if in_tmux {
        println!("  tmux blocks the kitty keyboard protocol; re-run outside tmux");
    }
}

// crossterm sets KeyEventKind on Windows unconditionally, but only on Unix
// once REPORT_EVENT_TYPES is pushed. Querying for the protocol there would
// answer "no" and cost us sustain on a platform that already has it.
pub fn detect_gating() -> Gating {
    if cfg!(windows) {
        return Gating::Held;
    }
    match crossterm::terminal::supports_keyboard_enhancement() {
        Ok(true) => Gating::Held,
        _ => Gating::Fixed(Duration::from_millis(400)),
    }
}
