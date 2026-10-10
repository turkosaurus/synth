use clap::{Parser, Subcommand};
use tracing_subscriber::fmt::format::FmtSpan;
use std::path::PathBuf;
use std::process::ExitCode;

use synth::input::keyboard::Gating;
use synth::{audio, script, terminal, view};

/// synth | rust+luaJIT terminal synthesizer
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Diagnose the environment, and a patch if one is given
    #[command(alias = "check")]
    Doctor {
        /// optional patch to lint
        path: Option<PathBuf>,
        /// sample rate to bake at; envelope lengths depend on it
        #[arg(long, default_value_t = 48000)]
        rate: u32,
    },
    /// Play a patch or sequence through the default output device
    Play { path: PathBuf },
    /// Play live from the keyboard; without a path, the default voice
    Tui { path: Option<PathBuf> },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

// never to sdtout to avoid collisions with the TUI; only when RUST_LOG is set
fn tracing_init() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        return Ok(());
    }
    let file = std::fs::File::create("synth.log")?;
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_span_events(FmtSpan::CLOSE)
        .with_ansi(false)
        .init();
    Ok(())
}

fn run() -> anyhow::Result<ExitCode> {
    tracing_init()?;
    let args = Args::parse();
    // the terminal reports a capability; the keyboard decides the policy
    let gating = if terminal::reports_key_release() {
        Gating::Held
    } else {
        Gating::Fixed(synth::input::keyboard::FIXED_NOTE)
    };
    match args.command {
        Commands::Doctor { path, rate } => {
            terminal::doctor(gating);
            // only patch problems fail; being in tmux is a fact, not an error
            if let Some(path) = path {
                println!();
                if !script::check(&path, rate)? {
                    return Ok(ExitCode::FAILURE);
                }
            }
        }
        Commands::Play { path } => audio::play(&path)?,
        Commands::Tui { path } => view::tui::run(path.as_deref(), gating)?,
    }
    Ok(ExitCode::SUCCESS)
}
