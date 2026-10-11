use clap::{Parser, Subcommand};
use tracing::debug;
use tracing_subscriber::fmt::format::FmtSpan;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

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
            view::report::environment(gating);
            // only patch problems fail; being in tmux is a fact, not an error
            if let Some(path) = path {
                println!();
                let patch = script::load(&path, rate)?;
                view::report::patch(&path, &patch, rate);
                if !patch.clean() {
                    return Ok(ExitCode::FAILURE);
                }
            }
        }
        Commands::Play { path } => play(&path)?,
        Commands::Tui { path } => view::tui::run(path.as_deref(), gating)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn play(path: &std::path::Path) -> anyhow::Result<()> {
    let output = audio::open()?;
    let (sample_rate, channels) = (output.sample_rate, output.channels);

    let patch = script::load(path, sample_rate)?;
    let notes = patch.timeline.events.len();
    let player = audio::Player::new(patch.timeline, patch.freqs, sample_rate as f32);
    let seconds = player.total_samples() as f64 / sample_rate as f64;

    let stats = Arc::new(audio::Stats::default());
    let _stream = audio::stream(&output, player, Arc::clone(&stats))?;

    println!("{notes} events · {seconds:.1}s · {sample_rate} Hz · {channels} ch");
    std::thread::sleep(Duration::from_secs_f64(seconds) + Duration::from_millis(300));

    // structured fields rather than a preformatted string, so the log is
    // queryable per counter. one snapshot feeds both the log and the print,
    // because taking a second one would read gauges that just reset.
    let snap = stats.snapshot();
    debug!(
        frames = snap.frames,
        clips = snap.clips,
        overruns = snap.overruns,
        errors = snap.errors,
        peak = snap.peak,
        budget_us = snap.budget_us,
        callback_us_max = snap.callback_us_max,
        load = snap.load,
        "stream",
    );
    println!("{}", view::report::stats(&snap));
    Ok(())
}
