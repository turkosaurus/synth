use clap::{Parser, Subcommand};
use crossterm::style::Stylize;
use crossterm::{
    cursor,
    event::{
        self, Event, KeyCode, KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute, terminal,
};
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// synth | rust+luaJIT terminal synthesizer
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run { path: PathBuf },
    Doctor,
    Tui,
}

#[derive(Clone, Copy)]
enum Gating {
    Held,
    Fixed(Duration),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    let gating = detect_gating();
    match args.command {
        Commands::Run { path } => run_lua(&path)?,
        Commands::Doctor => doctor(gating)?,
        Commands::Tui => tui(gating)?,
    }
    Ok(())
}

fn run_lua(path: &std::path::Path) -> mlua::Result<()> {
    let lua = mlua::Lua::new();
    lua.load(path).exec()
}

fn tui(gating: Gating) -> anyhow::Result<()> {
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

fn enable_gating(gating: Gating) -> io::Result<()> {
    if let Gating::Held = gating {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        )?;
    }
    Ok(())
}

fn disable_gating(gating: Gating) -> io::Result<()> {
    if let Gating::Held = gating {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
    }
    Ok(())
}

fn doctor(gating: Gating) -> io::Result<()> {
    let term = std::env::var("TERM").unwrap_or_default();
    let in_tmux = std::env::var("TMUX").is_ok();

    println!("{}", "Doctor".bold().blue());
    println!("{}{}", "     terminal:  ".bold(), term);
    println!(
        "{}{}",
        "  multiplexer:  ".bold(),
        if in_tmux { "tmux" } else { "none" }
    );

    match gating {
        Gating::Held => {
            println!("{}{}", "  key release:  ".bold(), "yes".green());
            println!("{}{}", "    gating:       ".bold(), "held notes");
        }
        Gating::Fixed(length) => {
            println!("{}{}", "  key release:  ".bold(), "no".red());
            println!(
                "{}{} {} ms",
                "       gating:  ".bold(),
                "fixed".yellow(),
                length.as_millis()
            );
        }
    }

    Ok(())
}

fn detect_gating() -> Gating {
    match crossterm::terminal::supports_keyboard_enhancement() {
        Ok(true) => Gating::Held,
        _ => Gating::Fixed(Duration::from_millis(400)),
    }
}
