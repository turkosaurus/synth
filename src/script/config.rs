//! `config.lua`: settings for the machine, read once at startup.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::Error;

use super::field;
use crate::spec::{Budget, CaptureConfig, Config, LogConfig, StreamName, TuiConfig};

/// `config.lua` under the config directory.
const FILE: [&str; 2] = ["synth", "config.lua"];
/// The custom folder under the data directory.
const CUSTOM: [&str; 2] = ["synth", "custom"];
/// The debug log under the state directory.
const LOG: [&str; 2] = ["synth", "log.jsonl"];
/// The live log under the runtime directory.
const SOCKET: [&str; 2] = ["synth", "log.sock"];

/// Where the config, data, and state directories are, from the environment. The XDG
/// variables win wherever they are set; otherwise Unix uses the XDG spec's
/// defaults under `HOME`, and Windows, which sets neither, uses `APPDATA`.
/// Values are taken as given so the rules can be tested on any platform.
struct Dirs {
    xdg_config: Option<OsString>,
    xdg_data: Option<OsString>,
    xdg_state: Option<OsString>,
    xdg_runtime: Option<OsString>,
    home: Option<OsString>,
    /// Windows' roaming application data, for config, the custom folder, and the log
    appdata: Option<OsString>,
    /// Windows' home, for `~/` when `HOME` is not set
    userprofile: Option<OsString>,
    windows: bool,
}

/// `value` as a path, if it is an absolute one. The XDG spec ignores a
/// relative value, and so does everything else here.
fn absolute(value: &Option<OsString>) -> Option<PathBuf> {
    value
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

impl Dirs {
    fn current() -> Self {
        Self {
            xdg_config: env::var_os("XDG_CONFIG_HOME"),
            xdg_data: env::var_os("XDG_DATA_HOME"),
            xdg_state: env::var_os("XDG_STATE_HOME"),
            xdg_runtime: env::var_os("XDG_RUNTIME_DIR"),
            home: env::var_os("HOME"),
            appdata: env::var_os("APPDATA"),
            userprofile: env::var_os("USERPROFILE"),
            windows: cfg!(windows),
        }
    }

    /// What `~/` stands for: `HOME`, else on Windows `USERPROFILE`.
    fn home(&self) -> Option<PathBuf> {
        absolute(&self.home).or_else(|| self.windows.then(|| absolute(&self.userprofile)).flatten())
    }

    /// `$XDG_CONFIG_HOME`, else `~/.config`, or `%APPDATA%` on Windows.
    fn config(&self) -> Option<PathBuf> {
        absolute(&self.xdg_config).or_else(|| match self.windows {
            true => absolute(&self.appdata),
            false => Some(absolute(&self.home)?.join(".config")),
        })
    }

    /// `$XDG_DATA_HOME`, else `~/.local/share`, or `%APPDATA%` on Windows.
    fn data(&self) -> Option<PathBuf> {
        absolute(&self.xdg_data).or_else(|| match self.windows {
            true => absolute(&self.appdata),
            false => Some(absolute(&self.home)?.join(".local").join("share")),
        })
    }

    /// `$XDG_STATE_HOME`, else `~/.local/state`, or `%APPDATA%` on Windows.
    fn state(&self) -> Option<PathBuf> {
        absolute(&self.xdg_state).or_else(|| match self.windows {
            true => absolute(&self.appdata),
            false => Some(absolute(&self.home)?.join(".local").join("state")),
        })
    }

    /// `$XDG_RUNTIME_DIR`, else `$XDG_STATE_HOME`, else `~/.local/state`,
    /// since no session manager sets a runtime directory on macOS. `None` on
    /// Windows, which has no Unix socket for it.
    fn runtime(&self) -> Option<PathBuf> {
        match self.windows {
            true => None,
            false => absolute(&self.xdg_runtime).or_else(|| self.state()),
        }
    }
}

fn under(dir: PathBuf, parts: &[&str]) -> PathBuf {
    parts.iter().fold(dir, |dir, part| dir.join(part))
}

/// The defaults, and what `synth config` writes when there is no file yet.
/// A user's file is laid over it, so it need set only what it changes.
const TEMPLATE: &str = include_str!("../../lua/config.lua");
const TEMPLATE_NAME: &str = "config.lua (built in)";
/// Stands in for the default custom folder in [`TEMPLATE`]'s comment.
const DEFAULT_CUSTOM_DIR: &str = "{default}";

/// `TEMPLATE` as `synth config` writes it, with the default custom
/// folder filled in, so the file shows a real path in the form to copy.
pub fn template() -> String {
    template_for(&Dirs::current())
}

/// The line naming the default is dropped when there is no data directory,
/// or when the path holds `]]` and would end the long string early.
fn template_for(dirs: &Dirs) -> String {
    let dir = custom_dir("", dirs).ok().flatten();
    match dir.map(|dir| dir.display().to_string()) {
        Some(dir) if !dir.contains("]]") => TEMPLATE.replace(DEFAULT_CUSTOM_DIR, &dir),
        _ => TEMPLATE
            .lines()
            .filter(|line| !line.contains(DEFAULT_CUSTOM_DIR))
            .map(|line| format!("{line}\n"))
            .collect(),
    }
}

/// Reads `config.lua` from the user's config directory over the defaults.
/// A missing file, or no directory to look in, is the defaults alone.
pub fn config() -> Result<Config, Error> {
    let user = match config_path() {
        Some(path) => read(&path)?.map(|source| (source, path.display().to_string())),
        None => None,
    };
    resolve(
        user.as_ref()
            .map(|(source, name)| (source.as_str(), name.as_str())),
    )
}

/// Where `config` looks, whether or not a file is there:
/// `$XDG_CONFIG_HOME/synth/config.lua`, else `~/.config/synth/config.lua`,
/// or `%APPDATA%\synth\config.lua` on Windows. `None` when the
/// environment names none.
pub fn config_path() -> Option<PathBuf> {
    Dirs::current().config().map(|dir| under(dir, &FILE))
}

/// Where the log goes: `$XDG_STATE_HOME/synth/log.jsonl`, else
/// `~/.local/state/synth/log.jsonl`, or `%APPDATA%\synth\log.jsonl` on
/// Windows. `None` when the environment names none.
pub fn log_path() -> Option<PathBuf> {
    Dirs::current().state().map(|dir| under(dir, &LOG))
}

/// Where the live log listens: `$XDG_RUNTIME_DIR/synth/log.sock`, private
/// to the user and cleared at logout, so no stale socket outlives the
/// session. Without it, `$XDG_STATE_HOME/synth/log.sock`, else
/// `~/.local/state/synth/log.sock`.
/// `None` when the environment names none, and always on Windows.
pub fn socket_path() -> Option<PathBuf> {
    Dirs::current().runtime().map(|dir| under(dir, &SOCKET))
}

/// The file's source, or `None` when there is no file.
fn read(path: &Path) -> Result<Option<String>, Error> {
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io("reading", path, error)),
    }
}

/// The defaults, with `user` (source and name) laid over them. Both run in
/// one bare Lua state: `config.lua` needs none of the `synth` modules.
/// The built-in file's settings alone, for tests elsewhere in the crate.
#[cfg(test)]
pub(crate) fn defaults() -> Config {
    resolve(None).expect("the built-in config.lua loads")
}

fn resolve(user: Option<(&str, &str)>) -> Result<Config, Error> {
    let lua = mlua::Lua::new();
    let base = table(&lua, TEMPLATE, TEMPLATE_NAME)?;
    if let Some((source, name)) = user {
        overlay(&base, &table(&lua, source, name)?)?;
    }
    settings(&base)
}

/// Runs a config file and checks the table it returns. Every field is
/// optional here, so a user's file is checked on its own terms and names
/// its own mistakes.
fn table(lua: &mlua::Lua, source: &str, name: &str) -> Result<mlua::Table, Error> {
    let returned: mlua::Value = super::sandbox::chunk(lua, source, name)
        .and_then(mlua::chunk::Chunk::eval)
        .map_err(|source| Error::Run {
            name: name.to_string(),
            source: super::error::trimmed(source),
        })?;
    let mlua::Value::Table(table) = returned else {
        return Err(Error::ReturnNotATable {
            what: name.to_string(),
            got: returned.type_name(),
        });
    };
    super::schema::CONFIG.check(&table, None)?;
    if let Some(capture) = table.get::<Option<mlua::Table>>(field::CAPTURE)? {
        super::schema::CONFIG_CAPTURE.check(&capture, None)?;
    }
    if let Some(log) = table.get::<Option<mlua::Table>>(field::LOG)? {
        super::schema::CONFIG_LOG.check(&log, None)?;
    }
    if let Some(tui) = table.get::<Option<mlua::Table>>(field::TUI)? {
        super::schema::CONFIG_TUI.check(&tui, None)?;
        if let Some(latency) = tui.get::<Option<mlua::Table>>(field::METER_LATENCY)? {
            super::schema::CONFIG_LATENCY.check(&latency, Some(LATENCY))?;
            for stage in STAGES {
                if let Some(budget) = latency.get::<Option<mlua::Table>>(stage)? {
                    super::schema::CONFIG_BUDGET.check(&budget, Some(&stage_name(stage)))?;
                }
            }
        }
        if let Some(fps) = tui.get::<Option<mlua::Table>>(field::METER_FPS)? {
            super::schema::CONFIG_BUDGET.check(&fps, Some(FPS))?;
        }
        if let Some(source) = tui.get::<Option<mlua::Table>>(field::SOURCE)? {
            super::schema::CONFIG_SOURCE.check(&source, None)?;
        }
        if let Some(scope) = tui.get::<Option<mlua::Table>>(field::SCOPE)? {
            super::schema::CONFIG_SCOPE.check(&scope, None)?;
        }
        if let Some(spectrum) = tui.get::<Option<mlua::Table>>(field::SPECTRUM)? {
            super::schema::CONFIG_SPECTRUM.check(&spectrum, None)?;
        }
    }
    Ok(table)
}

/// Copies each of `over`'s fields onto `base`, merging a nested table
/// field by field rather than replacing it.
fn overlay(base: &mlua::Table, over: &mlua::Table) -> mlua::Result<()> {
    for pair in over.pairs::<mlua::Value, mlua::Value>() {
        let (key, value) = pair?;
        match (base.get::<mlua::Value>(key.clone())?, value) {
            (mlua::Value::Table(inner), mlua::Value::Table(value)) => overlay(&inner, &value)?,
            (_, value) => base.set(key, value)?,
        }
    }
    Ok(())
}

/// The settings from a checked, complete table.
/// The custom folder `setting` names. `""` is `synth/custom` under the
/// data directory; any other value is an absolute path or one starting
/// `~/`. `None` when the default is asked for and there is no data directory.
fn custom_dir(setting: &str, dirs: &Dirs) -> Result<Option<PathBuf>, Error> {
    if setting.is_empty() {
        return Ok(dirs.data().map(|dir| under(dir, &CUSTOM)));
    }
    if let Some(rest) = setting.strip_prefix("~/") {
        let home = dirs.home().ok_or(Error::CustomDirNoHome)?;
        return Ok(Some(home.join(rest)));
    }
    let path = PathBuf::from(setting);
    if !path.is_absolute() {
        return Err(Error::CustomDirRelative {
            setting: setting.to_string(),
        });
    }
    Ok(Some(path))
}

/// `low` and `high` from `table`, once `low` is known to be below `high`.
/// `what` names the table in the error.
fn ordered(
    table: &mlua::Table,
    what: &str,
    low: &'static str,
    high: &'static str,
) -> Result<(f64, f64), Error> {
    let (low_value, high_value): (f64, f64) = (table.get(low)?, table.get(high)?);
    if low_value >= high_value {
        return Err(Error::ConfigOrder {
            what: what.to_string(),
            low,
            low_value,
            high,
            high_value,
        });
    }
    Ok((low_value, high_value))
}

/// The tables under `tui` and their names, for checks and errors.
const LATENCY: &str = "config.lua tui meter_latency";
const FPS: &str = "config.lua tui meter_fps";
const STAGES: [&str; 4] = [field::INPUT, field::RENDER, field::OUTPUT, field::TOTAL];

fn stage_name(stage: &str) -> String {
    format!("{LATENCY} {stage}")
}

/// A stage's budget, whose warn comes below its danger.
fn budget(latency: &mlua::Table, stage: &str) -> Result<Budget, Error> {
    let table: mlua::Table = latency.get(stage)?;
    let (warn, danger) = ordered(&table, &stage_name(stage), field::WARN, field::DANGER)?;
    Ok(Budget {
        warn: warn as f32,
        danger: danger as f32,
    })
}

fn stream(name: &str) -> Result<StreamName, Error> {
    StreamName::ALL
        .into_iter()
        .find(|stream| stream.name() == name)
        .ok_or_else(|| Error::UnknownStream {
            stream: name.to_string(),
            expected: StreamName::ALL.map(StreamName::name).join(", "),
        })
}

fn settings(table: &mlua::Table) -> Result<Config, Error> {
    let tui: mlua::Table = table.get(field::TUI)?;
    let ms = |key: &str| -> mlua::Result<Duration> {
        Ok(Duration::from_secs_f64(tui.get::<f64>(key)? / 1000.0))
    };
    let latency: mlua::Table = tui.get(field::METER_LATENCY)?;
    let (floor, ceil) = ordered(&latency, LATENCY, field::FLOOR, field::CEIL)?;
    // a frame rate is better higher, so its danger comes below its warn
    let fps: mlua::Table = tui.get(field::METER_FPS)?;
    let log: mlua::Table = table.get(field::LOG)?;
    let capture: mlua::Table = table.get(field::CAPTURE)?;
    let source: mlua::Table = tui.get(field::SOURCE)?;
    let scope: mlua::Table = tui.get(field::SCOPE)?;
    let spectrum: mlua::Table = tui.get(field::SPECTRUM)?;
    let size: usize = spectrum.get(field::SIZE)?;
    if !size.is_power_of_two() {
        return Err(Error::SpectrumSize { size });
    }
    let within = |table: &mlua::Table, key: &str| -> mlua::Result<Duration> {
        Ok(Duration::from_secs_f64(table.get::<f64>(key)? / 1000.0))
    };
    let (fps_danger, fps_warn) = ordered(&fps, FPS, field::DANGER, field::WARN)?;
    Ok(Config {
        // 0 asks for nothing, the same as leaving it out
        buffer_frames: table
            .get::<Option<u32>>(field::BUFFER_FRAMES)?
            .filter(|&frames| frames > 0),
        queue: table.get(field::QUEUE)?,
        // absent is the default; "" says the same thing explicitly
        custom_dir: custom_dir(
            &table
                .get::<Option<String>>(field::CUSTOM_DIR)?
                .unwrap_or_default(),
            &Dirs::current(),
        )?,
        capture: CaptureConfig {
            channel: capture.get(field::CHANNEL)?,
        },
        tui: TuiConfig {
            poll: ms(field::POLL)?,
            meter_hold: ms(field::METER_HOLD)?,
            meter_fall: ms(field::METER_FALL)?,
            meter_floor_db: tui.get(field::METER_FLOOR)?,
            latency_window: Duration::from_secs_f64(latency.get::<f64>(field::WINDOW)? / 1000.0),
            latency_records: latency.get(field::RECORDS)?,
            latency_floor_ms: floor as f32,
            latency_ceil_ms: ceil as f32,
            input_budget: budget(&latency, field::INPUT)?,
            render_budget: budget(&latency, field::RENDER)?,
            output_budget: budget(&latency, field::OUTPUT)?,
            total_budget: budget(&latency, field::TOTAL)?,
            fps: Budget {
                warn: fps_warn as f32,
                danger: fps_danger as f32,
            },
            faults: tui.get(field::FAULTS)?,
            source_stream: stream(&source.get::<String>(field::STREAM)?)?,
            source_channel: source.get(field::CHANNEL)?,
            source_tee: within(&source, field::TEE)?,
            scope_history: within(&scope, field::HISTORY)?,
            scope_cycles: scope.get(field::CYCLES)?,
            spectrum_size: size,
            spectrum_floor_db: spectrum.get(field::FLOOR)?,
            spectrum_height: spectrum.get(field::HEIGHT)?,
        },
        log: LogConfig {
            trim_size_kb: log.get(field::TRIM_SIZE_KB)?,
        },
    })
}

#[cfg(test)]
mod tests;
