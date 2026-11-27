//! `config.lua`: settings for the machine, read once at startup.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::Error;

use super::field;
use crate::spec::{Budget, Config, TuiConfig};

/// `config.lua` under the config directory.
const FILE: [&str; 2] = ["synth", "config.lua"];
/// The custom folder under the data directory.
const CUSTOM: [&str; 2] = ["synth", "custom"];
/// The debug log under the state directory.
const LOG: [&str; 2] = ["synth", "synth.log"];

/// Where the config, data, and state directories are, from the environment. The XDG
/// variables win wherever they are set; otherwise Unix uses the XDG spec's
/// defaults under `HOME`, and Windows, which sets neither, uses `APPDATA`.
/// Values are taken as given so the rules can be tested on any platform.
struct Dirs {
    xdg_config: Option<OsString>,
    xdg_data: Option<OsString>,
    xdg_state: Option<OsString>,
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

/// Where the log goes: `$XDG_STATE_HOME/synth/synth.log`, else
/// `~/.local/state/synth/synth.log`, or `%APPDATA%\synth\synth.log` on
/// Windows. `None` when the environment names none.
pub fn log_path() -> Option<PathBuf> {
    Dirs::current().state().map(|dir| under(dir, &LOG))
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

fn settings(table: &mlua::Table) -> Result<Config, Error> {
    let tui: mlua::Table = table.get(field::TUI)?;
    let ms = |key: &str| -> mlua::Result<Duration> {
        Ok(Duration::from_secs_f64(tui.get::<f64>(key)? / 1000.0))
    };
    let latency: mlua::Table = tui.get(field::METER_LATENCY)?;
    let (floor, ceil) = ordered(&latency, LATENCY, field::FLOOR, field::CEIL)?;
    // a frame rate is better higher, so its danger comes below its warn
    let fps: mlua::Table = tui.get(field::METER_FPS)?;
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
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `path`, a Unix-style absolute path, made absolute on the test host:
    /// Windows needs a drive, so `/h` becomes `C:/h` there.
    fn abs(path: &str) -> PathBuf {
        match cfg!(windows) {
            true => PathBuf::from(format!("C:{path}")),
            false => PathBuf::from(path),
        }
    }

    /// An environment value: absolute ones go through [`abs`], relative
    /// ones are passed as written.
    fn var(value: &str) -> OsString {
        match value.starts_with('/') {
            true => abs(value).into_os_string(),
            false => OsString::from(value),
        }
    }

    /// A Unix environment with these XDG and HOME values.
    fn unix(xdg: Option<&str>, home: Option<&str>) -> Dirs {
        Dirs {
            xdg_config: xdg.map(var),
            xdg_data: xdg.map(var),
            xdg_state: xdg.map(var),
            home: home.map(var),
            appdata: None,
            userprofile: None,
            windows: false,
        }
    }

    /// A Windows environment with these APPDATA and USERPROFILE values.
    fn windows(appdata: Option<&str>, userprofile: Option<&str>) -> Dirs {
        Dirs {
            xdg_config: None,
            xdg_data: None,
            xdg_state: None,
            home: None,
            appdata: appdata.map(var),
            userprofile: userprofile.map(var),
            windows: true,
        }
    }

    fn at(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        unix(xdg, home).config().map(|dir| under(dir, &FILE))
    }

    #[test]
    fn the_path_prefers_xdg() {
        assert_eq!(at(Some("/x"), Some("/h")), Some(abs("/x/synth/config.lua")));
        assert_eq!(
            at(None, Some("/h")),
            Some(abs("/h/.config/synth/config.lua"))
        );
    }

    #[test]
    fn the_path_ignores_relative_dirs() {
        assert_eq!(
            at(Some("x"), Some("/h")),
            Some(abs("/h/.config/synth/config.lua"))
        );
        assert_eq!(at(None, Some("h")), None);
        assert_eq!(at(None, None), None);
    }

    #[test]
    fn the_log_prefers_xdg_state() {
        let log = |xdg, home| unix(xdg, home).state().map(|dir| under(dir, &LOG));
        assert_eq!(log(Some("/x"), Some("/h")), Some(abs("/x/synth/synth.log")));
        assert_eq!(
            log(None, Some("/h")),
            Some(abs("/h/.local/state/synth/synth.log"))
        );
        assert_eq!(log(Some("x"), None), None);
    }

    fn user(source: &str) -> Result<Config, Error> {
        resolve(Some((source, "test")))
    }

    #[test]
    fn a_missing_file_reads_as_none() {
        assert_eq!(
            read(Path::new("/nonexistent/synth/config.lua")).unwrap(),
            None
        );
    }

    #[test]
    fn a_config_runs_in_the_sandbox() {
        let config = resolve(Some(("return { queue = os and 32 or 64 }", "test"))).unwrap();
        assert_eq!(config.queue, 64);
    }

    #[test]
    fn the_defaults_are_the_built_in_file() {
        let config = resolve(None).unwrap();
        assert_eq!(config.buffer_frames, None);
        assert_eq!(config.queue, 256);
        assert_eq!(config.tui.poll, Duration::from_millis(16));
        assert_eq!(config.tui.meter_hold, Duration::from_millis(560));
        assert_eq!(config.tui.meter_fall, Duration::from_millis(280));
        assert_eq!(config.tui.meter_floor_db, -60.0);
        assert_eq!(config.tui.latency_window, Duration::from_secs(5));
        assert_eq!(config.tui.latency_records, 1024);
        assert_eq!(config.tui.faults, 64);
        assert_eq!(config.tui.latency_floor_ms, 0.01);
        assert_eq!(config.tui.latency_ceil_ms, 100.0);
        let budget = |warn, danger| Budget { warn, danger };
        assert_eq!(config.tui.input_budget, budget(1.0, 5.0));
        assert_eq!(config.tui.render_budget, budget(5.0, 10.0));
        assert_eq!(config.tui.output_budget, budget(10.0, 20.0));
        assert_eq!(config.tui.total_budget, budget(10.0, 20.0));
        assert_eq!(config.tui.fps, budget(30.0, 24.0));
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        assert_eq!(user("return {}").unwrap(), resolve(None).unwrap());
    }

    #[test]
    fn a_file_changes_only_what_it_sets() {
        let config = user("return { queue = 512, tui = { meter_floor = -90 } }").unwrap();
        let defaults = resolve(None).unwrap();

        assert_eq!(config.queue, 512);
        assert_eq!(config.tui.meter_floor_db, -90.0);
        // the rest of `tui` is merged, not replaced
        assert_eq!(config.tui.poll, defaults.tui.poll);
        assert_eq!(config.tui.meter_hold, defaults.tui.meter_hold);
        assert_eq!(config.buffer_frames, defaults.buffer_frames);
    }

    #[test]
    fn a_nested_budget_changes_only_what_it_sets() {
        let config =
            user("return { tui = { meter_latency = { render = { warn = 6 } } } }").unwrap();
        let defaults = resolve(None).unwrap();
        assert_eq!(config.tui.render_budget.warn, 6.0);
        assert_eq!(
            config.tui.render_budget.danger,
            defaults.tui.render_budget.danger
        );
        assert_eq!(config.tui.latency_window, defaults.tui.latency_window);
    }

    #[test]
    fn zero_leaves_the_buffer_to_the_platform() {
        assert_eq!(
            user("return { buffer_frames = 0 }").unwrap().buffer_frames,
            None
        );
    }

    #[test]
    fn buffer_frames_is_read() {
        assert_eq!(
            user("return { buffer_frames = 128 }")
                .unwrap()
                .buffer_frames,
            Some(128)
        );
    }

    fn custom(setting: &str, xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        // an absolute setting is made absolute on the host, as `var` does
        let setting = match setting.starts_with('/') {
            true => abs(setting).display().to_string(),
            false => setting.to_string(),
        };
        custom_dir(&setting, &unix(xdg, home)).unwrap()
    }

    #[test]
    fn the_custom_folder_defaults_to_the_data_dir() {
        assert_eq!(
            custom("", Some("/d"), Some("/h")),
            Some(abs("/d/synth/custom"))
        );
        assert_eq!(
            custom("", None, Some("/h")),
            Some(abs("/h/.local/share/synth/custom"))
        );
        assert_eq!(custom("", Some("d"), None), None);
    }

    #[test]
    fn the_template_names_the_default_custom_folder() {
        // joined as the code joins it, so Windows gets its own separators
        let dir = abs("/d").join("synth").join("custom");
        let dir = dir.display().to_string();
        let written = template_for(&unix(Some("/d"), None));
        assert!(written.contains(&format!("-- custom_dir = [[{dir}]]")));
        assert!(!written.contains(DEFAULT_CUSTOM_DIR));

        let written = template_for(&unix(None, None));
        assert!(!written.contains("-- custom_dir ="));
    }

    #[test]
    fn the_custom_folder_takes_absolute_and_home_paths() {
        assert_eq!(
            custom("/music/synth", None, Some("/h")),
            Some(abs("/music/synth"))
        );
        assert_eq!(custom("~/music", None, Some("/h")), Some(abs("/h/music")));
    }

    #[test]
    fn windows_uses_appdata_for_config_and_custom() {
        let dirs = windows(Some("/appdata"), Some("/users/me"));
        assert_eq!(
            dirs.config().map(|dir| under(dir, &FILE)),
            Some(abs("/appdata/synth/config.lua"))
        );
        assert_eq!(
            custom_dir("", &dirs).unwrap(),
            Some(abs("/appdata/synth/custom"))
        );
        // with no HOME, ~/ is the user profile
        assert_eq!(
            custom_dir("~/music", &dirs).unwrap(),
            Some(abs("/users/me/music"))
        );
    }

    #[test]
    fn xdg_wins_on_windows_too() {
        let dirs = Dirs {
            xdg_config: Some(var("/x/config")),
            xdg_data: Some(var("/x/data")),
            ..windows(Some("/appdata"), None)
        };
        assert_eq!(dirs.config(), Some(abs("/x/config")));
        assert_eq!(dirs.data(), Some(abs("/x/data")));
    }

    #[test]
    fn userprofile_stands_in_for_home_only_on_windows() {
        let unix = Dirs {
            userprofile: Some(var("/users/me")),
            ..unix(None, None)
        };
        assert_eq!(unix.home(), None);
    }

    #[test]
    fn a_relative_custom_folder_is_named() {
        let error = custom_dir("music", &unix(None, Some("/h"))).unwrap_err();
        assert!(
            matches!(&error, Error::CustomDirRelative { setting } if setting == "music"),
            "{error}"
        );
        let error = custom_dir("~/music", &unix(None, None)).unwrap_err();
        assert!(matches!(error, Error::CustomDirNoHome), "{error}");
    }

    #[test]
    fn a_bad_value_is_named() {
        for (source, expected) in [
            ("return { buffer_frames = -1 }", "`buffer_frames` is -1"),
            (
                "return { buffer_frames = 2.5 }",
                "`buffer_frames` must be a whole number",
            ),
            (
                "return { buffer_frames = '256' }",
                "`buffer_frames` must be a whole number",
            ),
            (
                "return { bufer_frames = 256 }",
                "unknown field `bufer_frames`",
            ),
            ("return { queue = 8 }", "`queue` is 8; must be at least 16"),
            ("return { custom_dir = 5 }", "`custom_dir` must be a string"),
            ("return { tui = 16 }", "`tui` must be a table"),
            (
                "return { tui = { poll = 0 } }",
                "config.lua tui: `poll` is 0; must be 1 to 1000",
            ),
            (
                "return { tui = { meter_fall = 0 } }",
                "`meter_fall` is 0; must be above 0",
            ),
            (
                "return { tui = { meter_floor = 0 } }",
                "`meter_floor` is 0; must be below 0",
            ),
            (
                "return { tui = { meter_latency = { window = 0 } } }",
                "config.lua tui meter_latency: `window` is 0; must be above 0",
            ),
            (
                "return { tui = { meter_latency = { records = 0 } } }",
                "config.lua tui meter_latency: `records` is 0; must be 1 to 65536",
            ),
            (
                "return { tui = { faults = 0 } }",
                "config.lua tui: `faults` is 0; must be 1 to 65536",
            ),
            (
                "return { tui = { meter_latency = { records = 1.5 } } }",
                "`records` must be a whole number",
            ),
            (
                "return { tui = { meter_latency = { floor = 0 } } }",
                "`floor` is 0; must be above 0",
            ),
            (
                "return { tui = { meter_latency = { floor = 100 } } }",
                "config.lua tui meter_latency: `floor` is 100, `ceil` is 100; `floor` must be below `ceil`",
            ),
            (
                "return { tui = { meter_latency = { render = { warn = 10 } } } }",
                "config.lua tui meter_latency render: `warn` is 10, `danger` is 10; `warn` must be below `danger`",
            ),
            (
                "return { tui = { meter_latency = { input = { wran = 1 } } } }",
                "config.lua tui meter_latency input: unknown field `wran`",
            ),
            (
                "return { tui = { meter_fps = { danger = 30 } } }",
                "config.lua tui meter_fps: `danger` is 30, `warn` is 30; `danger` must be below `warn`",
            ),
            (
                "return { tui = { meter_latency = { windw = 1 } } }",
                "unknown field `windw`",
            ),
            ("return { tui = { pol = 16 } }", "unknown field `pol`"),
            ("return 256", "must return a table, got integer"),
        ] {
            let error = user(source).expect_err(source);
            assert!(
                format!("{error:#}").contains(expected),
                "{source}: {error:#}"
            );
        }
    }

    #[test]
    fn a_config_that_does_not_run_is_matched_by_variant() {
        let error = user("return {").unwrap_err();
        assert!(
            matches!(&error, Error::Run { name, .. } if name == "test"),
            "{error}"
        );
        let error = user("return 256").unwrap_err();
        assert!(
            matches!(&error, Error::ReturnNotATable { what, got: "integer" } if what == "test"),
            "{error}"
        );
    }
}
