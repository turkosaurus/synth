use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;
use tracing_subscriber::fmt::format::FmtSpan;

use synth::input::Gating;
use synth::session::Session;
use synth::{audio, install, release, script, terminal, update, view};

/// synth | rust+luaJIT terminal synthesizer
#[derive(Parser)]
#[command(version = release::VERSION)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Diagnose the environment, config, and built-in copies
    Doctor,
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
    /// Open config.lua in $VISUAL or $EDITOR, creating it if needed
    Config,
    /// Open your custom folder in $VISUAL or $EDITOR, creating it and
    /// copying in the built-in modules if needed
    Custom {
        /// print the folder's path instead, for editors that cannot open one
        #[arg(long, conflicts_with = "sync")]
        path: bool,
        /// bring the built-in copies up to date, asking about edited ones,
        /// without opening the editor; `update` runs this
        #[arg(long)]
        sync: bool,
    },
    /// Copy this binary to $XDG_BIN_HOME, or ~/.local/bin
    Install,
    /// Remove this binary
    Uninstall,
    /// Install the latest build of a release channel from GitHub
    Update {
        #[command(flatten)]
        channel: ChannelArgs,
        /// download, verify, and unpack the build, but leave this binary
        #[arg(long)]
        dry_run: bool,
    },
    /// Print the version, as --version does
    Version,
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
    #[arg(
        long,
        value_name = "SHA",
        num_args = 0..=1,
        default_missing_value = update::DEV_NEWEST
    )]
    dev: Option<String>,
}

impl ChannelArgs {
    fn channel(self) -> update::Channel {
        match (self.beta, self.dev) {
            (true, _) => update::Channel::Beta,
            (_, Some(sha)) if sha == update::DEV_NEWEST => update::Channel::Dev(None),
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
    let path = script::log_path()
        .context("no log directory: set XDG_STATE_HOME or HOME, or APPDATA on Windows")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let file =
        std::fs::File::create(&path).with_context(|| format!("create {}", path.display()))?;
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
        Commands::Doctor => {
            view::report::environment(gating());
            println!();
            // a config that does not load is reported as the error it is
            let config_path = script::config_path();
            let config = script::config()?;
            view::report::config(config_path.as_deref(), &config);
            if let Some(custom) = &config.custom_dir {
                println!();
                view::report::builtins(&script::builtins::states(custom));
            }
        }
        Commands::Check { path, rate } => return lint(&path, rate),
        Commands::Play { path } => play(&path, &script::config()?)?,
        Commands::Tui { path } => {
            let config = script::config()?;
            let mut session = Session::open(path.as_deref(), gating(), &config)?;
            view::tui::run(&mut session, &config.tui)?;
        }
        Commands::Config => return edit_config(),
        Commands::Custom { path, sync } => custom(path, sync)?,
        Commands::Install => match install::install()? {
            install::Installed::Already(to) => println!("synth is installed at {}", to.display()),
            install::Installed::Copied { to, on_path } => {
                println!("installed synth {} to {}", release::VERSION, to.display());
                if !on_path {
                    let dir = to.parent().unwrap_or(&to).display();
                    if cfg!(windows) {
                        eprintln!(
                            "{dir} is not on PATH; add it under Settings, \
                             Environment Variables, then open a new terminal"
                        );
                    } else {
                        eprintln!("{dir} is not on PATH; add it, e.g. export PATH=\"{dir}:$PATH\"");
                    }
                }
            }
        },
        Commands::Uninstall => println!("removed {}", install::uninstall()?.display()),
        Commands::Update { channel, dry_run } => match update::run(&channel.channel(), dry_run)? {
            update::Outcome::Current(version) => println!("synth {version} is current"),
            update::Outcome::Installed { from, to, path } => {
                println!("synth {from} -> {to}");
                sync_custom(&path);
            }
            update::Outcome::Checked { to, path } => {
                println!("synth {to} verified; would replace {}", path.display());
            }
        },
        Commands::Version => print!("{}", Args::command().render_version()),
    }
    Ok(ExitCode::SUCCESS)
}

/// The terminal reports a capability; the keyboard decides the policy.
/// Probing switches the terminal to raw mode, which stops the process when
/// it runs in a background job, as under `mise run test:local`, so only the commands
/// that need it ask.
fn gating() -> Gating {
    if terminal::reports_key_release() {
        Gating::Held
    } else {
        Gating::Fixed
    }
}

/// Opens `config.lua` in the user's editor, writing the template first if
/// there is no file, then checks what was saved.
fn edit_config() -> anyhow::Result<ExitCode> {
    let path = script::config_path()
        .context("no config directory: set XDG_CONFIG_HOME or HOME, or APPDATA on Windows")?;
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        std::fs::write(&path, script::config_template())
            .with_context(|| format!("write {}", path.display()))?;
    }

    open_in_editor(&path)?;

    match script::config() {
        Ok(config) => {
            view::report::config(Some(&path), &config);
            Ok(ExitCode::SUCCESS)
        }
        // saved as written; the next start reports the same error
        Err(error) => {
            eprintln!("error: {error:#}");
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Prints the custom folder, or creates it with a folder per kind and
/// opens it in the user's editor.
fn custom(print_path: bool, sync: bool) -> anyhow::Result<()> {
    let dir = script::config()?.custom_dir.context(
        "no custom folder: set `custom_dir` in config.lua, or HOME, or APPDATA on Windows",
    )?;
    if print_path {
        println!("{}", dir.display());
        return Ok(());
    }
    for wrote in script::starters::write(&dir)? {
        println!("wrote {wrote}");
    }
    let wrote = script::builtins::write(&dir)?;
    // a question needs someone at a terminal to answer it; a script or CI
    // run is told which copies are behind and left to decide later
    let asking = std::io::stdin().is_terminal();
    view::report::builtins_written(&wrote, asking);
    if asking {
        resolve_behind(&dir, &wrote)?;
    }
    if sync {
        return Ok(());
    }
    open_in_editor(&dir)
}

/// Asks about each edited copy whose built-in changed since it was written,
/// and overwrites or keeps it as answered.
fn resolve_behind(
    dir: &std::path::Path,
    wrote: &[(&str, script::builtins::Wrote)],
) -> anyhow::Result<()> {
    use script::builtins::{self, Wrote};
    use view::prompt::Choice;

    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout();
    // escape codes only for a terminal; crossterm also drops them for
    // NO_COLOR
    let color = output.is_terminal();
    for (name, outcome) in wrote {
        if *outcome != Wrote::KeptBehind {
            continue;
        }
        let Some(stale) = builtins::behind(dir, name)? else {
            continue;
        };
        let choice = view::prompt::overwrite(
            &stale.path,
            &stale.copy,
            stale.built_in,
            color,
            &mut input,
            &mut output,
        )?;
        match choice {
            Choice::Overwrite => {
                builtins::overwrite(dir, &stale)?;
                println!("overwrote {name} with the new default");
            }
            Choice::Keep => {
                builtins::keep(dir, &stale)?;
                println!("kept {name}");
            }
        }
    }
    Ok(())
}

/// After an update, runs `custom --sync` from the new binary at `exe`, so built-in
/// copies follow the new built-ins and edited ones are asked about. Only
/// for a custom folder that already has copies; a failure here is
/// reported, but the update itself has succeeded.
fn sync_custom(exe: &std::path::Path) {
    let Ok(Some(dir)) = script::config().map(|config| config.custom_dir) else {
        return;
    };
    if !script::builtins::written(&dir) {
        return;
    }
    // current_exe would name the replaced binary, which Linux reports as
    // deleted
    let ran = std::process::Command::new(exe)
        .args(["custom", "--sync"])
        .status();
    match ran {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("synth custom --sync exited with {status}; run it again"),
        Err(error) => eprintln!("could not run synth custom --sync: {error}"),
    }
}

/// Opens `path`, a file or a folder, in the user's editor and waits for it.
fn open_in_editor(path: &std::path::Path) -> anyhow::Result<()> {
    let command = editor(std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok());
    let (program, args) = command.split_first().context("empty editor command")?;
    let status = std::process::Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .with_context(|| format!("run {}", command.join(" ")))?;
    anyhow::ensure!(
        status.success(),
        "{} exited with {status}",
        command.join(" ")
    );
    Ok(())
}

/// `$VISUAL`, else `$EDITOR`, else `vi`, split on whitespace so a value
/// like `code --wait` passes its flag. An empty variable counts as unset.
fn editor(visual: Option<String>, editor: Option<String>) -> Vec<String> {
    let chosen = [visual, editor]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string());
    chosen.split_whitespace().map(str::to_string).collect()
}

/// Bakes a patch and reports it; fails when the patch has warnings.
fn lint(path: &std::path::Path, rate: u32) -> anyhow::Result<ExitCode> {
    let config = script::config()?;
    let patch = script::load(path, rate, config.custom_dir.as_deref())?;
    view::report::patch(path, &patch, rate);
    if patch.clean() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

fn play(path: &std::path::Path, config: &synth::spec::Config) -> anyhow::Result<()> {
    let output = audio::open(config.buffer_frames)?;
    let (sample_rate, channels) = (output.sample_rate, output.channels);

    let patch = script::load(path, sample_rate, config.custom_dir.as_deref())?;
    let notes = patch.timeline.events.len();
    let player = audio::Player::new(
        audio::Feed::Timeline(patch.timeline),
        patch.freqs,
        sample_rate as f32,
    )
    .with_filter(patch.filter);
    let seconds = player.total_samples() as f64 / sample_rate as f64;

    let stats = Arc::new(audio::Stats::default());
    let _stream = audio::stream(&output, player, Arc::clone(&stats))?;

    // stdout carries only data, so these lines never mix into a pipe
    eprintln!("{notes} events · {seconds:.1}s · {sample_rate} Hz · {channels} ch");
    std::thread::sleep(Duration::from_secs_f64(seconds) + Duration::from_millis(300));

    // structured fields rather than a preformatted string, so the log is
    // queryable per counter. one snapshot feeds both the log and the print,
    // because taking a second one would read gauges that just reset.
    let snap = stats.snapshot();
    debug!(
        frames = snap.frames,
        clips = snap.clips,
        non_finite = snap.non_finite,
        overruns = snap.overruns,
        errors = snap.errors,
        dropped = snap.dropped,
        peak = snap.peak,
        budget_us = snap.budget_us,
        callback_us_max = snap.callback_us_max,
        load = snap.load,
        "stream",
    );
    eprintln!("{}", view::report::stats(&snap));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{editor, Args};
    use clap::Parser;

    #[test]
    fn doctor_takes_no_file() {
        assert!(Args::try_parse_from(["synth", "doctor"]).is_ok());
        assert!(Args::try_parse_from(["synth", "doctor", "patch.lua"]).is_err());
    }

    fn some(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn visual_wins_then_editor_then_vi() {
        assert_eq!(editor(some("nvim"), some("nano")), ["nvim"]);
        assert_eq!(editor(None, some("nano")), ["nano"]);
        assert_eq!(editor(some(" "), some("nano")), ["nano"]);
        assert_eq!(editor(None, None), ["vi"]);
    }

    #[test]
    fn an_editor_keeps_its_flags() {
        assert_eq!(editor(some("code --wait"), None), ["code", "--wait"]);
    }
}
