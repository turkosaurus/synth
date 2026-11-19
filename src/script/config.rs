//! `config.lua`: settings for the machine, read once at startup.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;

use super::field;
use crate::spec::{Config, TuiConfig};

/// `config.lua` under the config directory.
const FILE: [&str; 2] = ["synth", "config.lua"];
/// The patches folder under the data directory.
const PATCHES: [&str; 2] = ["synth", "patches"];

/// Where the config and data directories are, from the environment. The XDG
/// variables win wherever they are set; otherwise Unix uses the XDG spec's
/// defaults under `HOME`, and Windows, which sets neither, uses `APPDATA`.
/// Values are taken as given so the rules can be tested on any platform.
struct Dirs {
    xdg_config: Option<OsString>,
    xdg_data: Option<OsString>,
    home: Option<OsString>,
    /// Windows' roaming application data, for both config and patches
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
}

fn under(dir: PathBuf, parts: &[&str]) -> PathBuf {
    parts.iter().fold(dir, |dir, part| dir.join(part))
}

/// The defaults, and what `synth config` writes when there is no file yet.
/// A user's file is laid over it, so it need set only what it changes.
pub const TEMPLATE: &str = include_str!("../../lua/config.lua");
const TEMPLATE_NAME: &str = "config.lua (built in)";

/// Reads `config.lua` from the user's config directory over the defaults.
/// A missing file, or no directory to look in, is the defaults alone.
pub fn config() -> anyhow::Result<Config> {
    let user = match config_path() {
        Some(path) => read(&path)?.map(|source| (source, path.display().to_string())),
        None => None,
    };
    resolve(
        user.as_ref()
            .map(|(source, name)| (source.as_str(), name.as_str())),
    )
}

/// Where `config` looks, whether or not a file is there: `synth/config.lua`
/// under the config directory. `None` when the environment names none.
pub fn config_path() -> Option<PathBuf> {
    Dirs::current().config().map(|dir| under(dir, &FILE))
}

/// The file's source, or `None` when there is no file.
fn read(path: &Path) -> anyhow::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

/// The defaults, with `user` (source and name) laid over them. Both run in
/// one bare Lua state: `config.lua` needs none of the `synth` modules.
fn resolve(user: Option<(&str, &str)>) -> anyhow::Result<Config> {
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
fn table(lua: &mlua::Lua, source: &str, name: &str) -> anyhow::Result<mlua::Table> {
    let returned: mlua::Value = lua
        .load(source)
        .set_name(name)
        .eval()
        .with_context(|| format!("running {name}"))?;
    let mlua::Value::Table(table) = returned else {
        anyhow::bail!("{name} must return a table, got {}", returned.type_name());
    };
    super::schema::CONFIG.check(&table, None)?;
    if let Some(tui) = table.get::<Option<mlua::Table>>(field::TUI)? {
        super::schema::CONFIG_TUI.check(&tui, None)?;
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
/// The patches folder `setting` names. `""` is `synth/patches` under the
/// data directory; any other value is an absolute path or one starting
/// `~/`. `None` when the default is asked for and there is no data directory.
fn patches_dir(setting: &str, dirs: &Dirs) -> anyhow::Result<Option<PathBuf>> {
    if setting.is_empty() {
        return Ok(dirs.data().map(|dir| under(dir, &PATCHES)));
    }
    if let Some(rest) = setting.strip_prefix("~/") {
        let home = dirs
            .home()
            .context("config.lua: `patches` starts with ~/, but there is no home directory")?;
        return Ok(Some(home.join(rest)));
    }
    let path = PathBuf::from(setting);
    anyhow::ensure!(
        path.is_absolute(),
        "config.lua: `patches` is {setting:?}; must be absolute, start with ~/, or be \"\""
    );
    Ok(Some(path))
}

fn settings(table: &mlua::Table) -> anyhow::Result<Config> {
    let tui: mlua::Table = table.get(field::TUI)?;
    let ms = |key: &str| -> mlua::Result<Duration> {
        Ok(Duration::from_secs_f64(tui.get::<f64>(key)? / 1000.0))
    };
    Ok(Config {
        // 0 asks for nothing, the same as leaving it out
        buffer_frames: table
            .get::<Option<u32>>(field::BUFFER_FRAMES)?
            .filter(|&frames| frames > 0),
        queue: table.get(field::QUEUE)?,
        patches: patches_dir(&table.get::<String>(field::PATCHES)?, &Dirs::current())?,
        tui: TuiConfig {
            poll: ms(field::POLL)?,
            meter_hold: ms(field::METER_HOLD)?,
            meter_fall: ms(field::METER_FALL)?,
            meter_floor_db: tui.get(field::METER_FLOOR)?,
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

    fn user(source: &str) -> anyhow::Result<Config> {
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
    fn the_defaults_are_the_built_in_file() {
        let config = resolve(None).unwrap();
        assert_eq!(config.buffer_frames, None);
        assert_eq!(config.queue, 256);
        assert_eq!(config.tui.poll, Duration::from_millis(16));
        assert_eq!(config.tui.meter_hold, Duration::from_millis(560));
        assert_eq!(config.tui.meter_fall, Duration::from_millis(280));
        assert_eq!(config.tui.meter_floor_db, -60.0);
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

    fn patches(setting: &str, xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        patches_dir(setting, &unix(xdg, home)).unwrap()
    }

    #[test]
    fn the_patches_folder_defaults_to_the_data_dir() {
        assert_eq!(
            patches("", Some("/d"), Some("/h")),
            Some(abs("/d/synth/patches"))
        );
        assert_eq!(
            patches("", None, Some("/h")),
            Some(abs("/h/.local/share/synth/patches"))
        );
        assert_eq!(patches("", Some("d"), None), None);
    }

    #[test]
    fn the_patches_folder_takes_absolute_and_home_paths() {
        assert_eq!(
            patches("/music/synth", None, Some("/h")),
            Some(abs("/music/synth"))
        );
        assert_eq!(patches("~/music", None, Some("/h")), Some(abs("/h/music")));
    }

    #[test]
    fn windows_uses_appdata_for_config_and_patches() {
        let dirs = windows(Some("/appdata"), Some("/users/me"));
        assert_eq!(
            dirs.config().map(|dir| under(dir, &FILE)),
            Some(abs("/appdata/synth/config.lua"))
        );
        assert_eq!(
            patches_dir("", &dirs).unwrap(),
            Some(abs("/appdata/synth/patches"))
        );
        // with no HOME, ~/ is the user profile
        assert_eq!(
            patches_dir("~/music", &dirs).unwrap(),
            Some(abs("/users/me/music"))
        );
    }

    #[test]
    fn xdg_wins_on_windows_too() {
        let dirs = Dirs {
            xdg_config: Some(abs("/x/config")),
            xdg_data: Some(abs("/x/data")),
            ..windows(Some("/appdata"), None)
        };
        assert_eq!(dirs.config(), Some(abs("/x/config")));
        assert_eq!(dirs.data(), Some(abs("/x/data")));
    }

    #[test]
    fn userprofile_stands_in_for_home_only_on_windows() {
        let unix = Dirs {
            userprofile: Some(abs("/users/me")),
            ..unix(None, None)
        };
        assert_eq!(unix.home(), None);
    }

    #[test]
    fn a_relative_patches_folder_is_named() {
        let error = patches_dir("music", &unix(None, Some("/h"))).unwrap_err();
        assert!(
            error.to_string().contains("`patches` is \"music\""),
            "{error}"
        );
        let error = patches_dir("~/music", &unix(None, None)).unwrap_err();
        assert!(error.to_string().contains("no home directory"), "{error}");
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
            ("return { patches = 5 }", "`patches` must be a string"),
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
}
