use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;
use tracing_subscriber::fmt::format::FmtSpan;

use synth::input::Gating;
use synth::session::Session;
use synth::{audio, release, script, terminal, update, view};

/// synth | rust+luaJIT terminal synthesizer
#[derive(Parser)]
#[command(version = release::VERSION)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Diagnose the environment, and a patch if one is given
    Doctor {
        /// optional patch to lint
        path: Option<PathBuf>,
        /// sample rate to bake at; envelope lengths depend on it
        #[arg(long, default_value_t = 48000)]
        rate: u32,
    },
    /// Lint a patch without touching the terminal, for scripts and CI
    Check {
        /// patch to lint
        path: PathBuf,
        /// sample rate to bake at; envelope lengths depend on it
        #[arg(long, default_value_t = 48000)]
        rate: u32,
    },
    /// Play a patch or sequence through the default output device
    Play { path: PathBuf },
    /// Play live from the keyboard; without a path, the default instrument
    Tui { path: Option<PathBuf> },
    /// Install the latest build of a release channel from GitHub
    Update {
        #[command(flatten)]
        channel: ChannelArgs,
    },
}

/// At most one channel; none means stable.
#[derive(clap::Args)]
#[group(multiple = false)]
struct ChannelArgs {
    /// the latest release (default)
    #[arg(long)]
    stable: bool,
    /// the latest beta while it is newer than stable, else dev
    #[arg(long)]
    beta: bool,
    /// the newest dev build, or the one built from SHA
    #[arg(long, value_name = "SHA", num_args = 0..=1, default_missing_value = "main")]
    dev: Option<String>,
}

impl ChannelArgs {
    fn channel(self) -> update::Channel {
        match (self.beta, self.dev) {
            (true, _) => update::Channel::Beta,
            (_, Some(sha)) if sha == "main" => update::Channel::Dev(None),
            (_, Some(sha)) => update::Channel::Dev(Some(sha)),
            _ => update::Channel::Stable,
        }
    }
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
    match args.command {
        Commands::Doctor { path, rate } => {
            view::report::environment(gating());
            // only patch problems fail; being in tmux is a fact, not an error
            if let Some(path) = path {
                println!();
                return lint(&path, rate);
            }
        }
        Commands::Check { path, rate } => return lint(&path, rate),
        Commands::Play { path } => play(&path)?,
        Commands::Tui { path } => {
            let mut session = Session::open(path.as_deref(), gating())?;
            view::tui::run(&mut session)?;
        }
        Commands::Update { channel } => match update::run(&channel.channel())? {
            update::Outcome::Current(version) => println!("synth {version} is current"),
            update::Outcome::Installed { from, to } => println!("synth {from} -> {to}"),
        },
    }
    Ok(ExitCode::SUCCESS)
}

/// The terminal reports a capability; the keyboard decides the policy.
/// Probing switches the terminal to raw mode, which stops the process when
/// it runs in a background job, as under `mise test`, so only the commands
/// that need it ask.
fn gating() -> Gating {
    if terminal::reports_key_release() {
        Gating::Held
    } else {
        Gating::Fixed
    }
}

/// Bakes a patch and reports it; fails when the patch has warnings.
fn lint(path: &std::path::Path, rate: u32) -> anyhow::Result<ExitCode> {
    let patch = script::load(path, rate)?;
    view::report::patch(path, &patch, rate);
    if patch.clean() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

fn play(path: &std::path::Path) -> anyhow::Result<()> {
    let output = audio::open()?;
    let (sample_rate, channels) = (output.sample_rate, output.channels);

    let patch = script::load(path, sample_rate)?;
    let notes = patch.timeline.events.len();
    let player = audio::Player::new(patch.timeline, patch.freqs, sample_rate as f32)
        .with_filter(patch.filter);
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
