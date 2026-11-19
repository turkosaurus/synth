//! `config.lua`: settings for the machine, read once at startup.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::field;
use crate::spec::Config;

const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";
const HOME: &str = "HOME";
/// The XDG spec's default config directory, under `$HOME`.
const DEFAULT_CONFIG_HOME: &str = ".config";
const FILE: [&str; 2] = ["synth", "config.lua"];

/// What `synth config` writes when there is no file yet: every setting,
/// commented out, so it starts as the default config.
pub const TEMPLATE: &str = include_str!("../../lua/config.lua");

/// Reads `config.lua` from the user's config directory. A missing file,
/// or no directory to look in, is the default config.
pub fn config() -> anyhow::Result<Config> {
    match config_path() {
        Some(path) => load(&path),
        None => Ok(Config::default()),
    }
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

fn load(path: &Path) -> anyhow::Result<Config> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    parse(&source, &path.display().to_string())
}

/// `config.lua` returns a plain table. It needs none of the `synth`
/// modules, so it runs in a bare Lua state.
fn parse(source: &str, name: &str) -> anyhow::Result<Config> {
    let lua = mlua::Lua::new();
    let returned: mlua::Value = lua
        .load(source)
        .set_name(name)
        .eval()
        .with_context(|| format!("running {name}"))?;
    let mlua::Value::Table(table) = returned else {
        anyhow::bail!("{name} must return a table, got {}", returned.type_name());
    };
    super::schema::CONFIG.check(&table, None)?;

    Ok(Config {
        buffer_frames: table.get(field::BUFFER_FRAMES)?,
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

    #[test]
    fn a_missing_file_is_the_default() {
        let config = load(Path::new("/nonexistent/synth/config.lua")).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn an_empty_table_leaves_the_buffer_to_the_platform() {
        assert_eq!(parse("return {}", "test").unwrap().buffer_frames, None);
    }

    #[test]
    fn the_template_is_the_default_config() {
        assert_eq!(parse(TEMPLATE, "template").unwrap(), Config::default());
    }

    #[test]
    fn buffer_frames_is_read() {
        let config = parse("return { buffer_frames = 128 }", "test").unwrap();
        assert_eq!(config.buffer_frames, Some(128));
    }

    #[test]
    fn a_bad_value_is_named() {
        for (source, expected) in [
            ("return { buffer_frames = 0 }", "`buffer_frames` is 0"),
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
            ("return 256", "must return a table, got integer"),
        ] {
            let error = parse(source, "test").expect_err(source);
            assert!(
                format!("{error:#}").contains(expected),
                "{source}: {error:#}"
            );
        }
    }
}
