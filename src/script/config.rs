//! `config.lua`: settings for the machine, read once at startup.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;

use super::field;
use crate::spec::{Config, TuiConfig};

const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";
const HOME: &str = "HOME";
/// The XDG spec's default config directory, under `$HOME`.
const DEFAULT_CONFIG_HOME: &str = ".config";
const FILE: [&str; 2] = ["synth", "config.lua"];
const XDG_DATA_HOME: &str = "XDG_DATA_HOME";
/// The XDG spec's default data directory, under `$HOME`.
const DEFAULT_DATA_HOME: [&str; 2] = [".local", "share"];
/// The patches folder under the data directory.
const PATCHES: [&str; 2] = ["synth", "patches"];

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

/// Where `config` looks, whether or not a file is there. `None` when
/// neither `XDG_CONFIG_HOME` nor `HOME` is an absolute path.
pub fn config_path() -> Option<PathBuf> {
    path(env::var_os(XDG_CONFIG_HOME), env::var_os(HOME))
}

/// `$XDG_CONFIG_HOME/synth/config.lua`, else `~/.config/synth/config.lua`.
/// The XDG spec ignores a relative `XDG_CONFIG_HOME`.
fn path(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let dir = xdg
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            let home = home.map(PathBuf::from).filter(|p| p.is_absolute())?;
            Some(home.join(DEFAULT_CONFIG_HOME))
        })?;
    Some(FILE.iter().fold(dir, |dir, part| dir.join(part)))
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
/// The patches folder `setting` names. `""` is the default,
/// `$XDG_DATA_HOME/synth/patches` else `~/.local/share/synth/patches`; any
/// other value is an absolute path or one starting `~/`. `None` when the
/// default is asked for and there is no home to put it in.
fn patches_dir(
    setting: &str,
    xdg_data: Option<OsString>,
    home: Option<OsString>,
) -> anyhow::Result<Option<PathBuf>> {
    let home = home.map(PathBuf::from).filter(|p| p.is_absolute());
    if setting.is_empty() {
        let data = xdg_data
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                Some(
                    DEFAULT_DATA_HOME
                        .iter()
                        .fold(home?, |dir, part| dir.join(part)),
                )
            });
        return Ok(data.map(|data| PATCHES.iter().fold(data, |dir, part| dir.join(part))));
    }
    if let Some(rest) = setting.strip_prefix("~/") {
        let home = home.context("config.lua: `patches` starts with ~/, but HOME is not set")?;
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
        patches: patches_dir(
            &table.get::<String>(field::PATCHES)?,
            env::var_os(XDG_DATA_HOME),
            env::var_os(HOME),
        )?,
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

    fn at(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        path(xdg.map(OsString::from), home.map(OsString::from))
    }

    #[test]
    fn the_path_prefers_xdg() {
        assert_eq!(
            at(Some("/x"), Some("/h")),
            Some("/x/synth/config.lua".into())
        );
        assert_eq!(
            at(None, Some("/h")),
            Some("/h/.config/synth/config.lua".into())
        );
    }

    #[test]
    fn the_path_ignores_relative_dirs() {
        assert_eq!(
            at(Some("x"), Some("/h")),
            Some("/h/.config/synth/config.lua".into())
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
        patches_dir(setting, xdg.map(OsString::from), home.map(OsString::from)).unwrap()
    }

    #[test]
    fn the_patches_folder_defaults_to_the_data_dir() {
        assert_eq!(
            patches("", Some("/d"), Some("/h")),
            Some("/d/synth/patches".into())
        );
        assert_eq!(
            patches("", None, Some("/h")),
            Some("/h/.local/share/synth/patches".into())
        );
        assert_eq!(patches("", Some("d"), None), None);
    }

    #[test]
    fn the_patches_folder_takes_absolute_and_home_paths() {
        assert_eq!(
            patches("/music/synth", None, Some("/h")),
            Some("/music/synth".into())
        );
        assert_eq!(
            patches("~/music", None, Some("/h")),
            Some("/h/music".into())
        );
    }

    #[test]
    fn a_relative_patches_folder_is_named() {
        let error = patches_dir("music", None, Some("/h".into())).unwrap_err();
        assert!(
            error.to_string().contains("`patches` is \"music\""),
            "{error}"
        );
        let error = patches_dir("~/music", None, None).unwrap_err();
        assert!(error.to_string().contains("HOME is not set"), "{error}");
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
