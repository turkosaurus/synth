//! `synth install` and `synth uninstall`: put the running binary on PATH,
//! or take it off. `update` swaps binaries with the same `place`.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};

/// What `install` did.
#[derive(Debug)]
pub enum Installed {
    /// The running binary is the installed one.
    Already(PathBuf),
    /// The running binary was copied to `to`.
    Copied { to: PathBuf, on_path: bool },
}

/// The running binary, resolved through symlinks,
/// so a symlink on PATH acts on the file it points to.
pub(crate) fn exe() -> anyhow::Result<PathBuf> {
    env::current_exe()
        .and_then(fs::canonicalize)
        .context("find the running binary")
}

/// `$XDG_BIN_HOME`, else `~/.local/bin`.
/// The XDG spec names only `~/.local/bin`; `XDG_BIN_HOME` is not in it,
/// but installers such as uv honor it, and the spec ignores relative paths.
fn bin_dir(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let xdg = xdg.map(PathBuf::from).filter(|p| p.is_absolute());
    xdg.or_else(|| {
        let home = home.map(PathBuf::from).filter(|p| p.is_absolute())?;
        Some(home.join(".local").join("bin"))
    })
}

/// Copies the running binary into the user's bin directory.
pub fn install() -> anyhow::Result<Installed> {
    let exe = exe()?;
    let Some(dir) = bin_dir(env::var_os("XDG_BIN_HOME"), env::var_os("HOME")) else {
        bail!("neither XDG_BIN_HOME nor HOME is an absolute path");
    };
    let to = dir.join("synth");
    if fs::canonicalize(&to).is_ok_and(|installed| installed == exe) {
        return Ok(Installed::Already(to));
    }
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    place(&exe, &to)?;

    let on_path = env::var_os("PATH").is_some_and(|path| env::split_paths(&path).any(|d| d == dir));
    Ok(Installed::Copied { to, on_path })
}

/// Removes the running binary, wherever it was installed.
/// synth keeps no other files yet; when it does, they go here too.
pub fn uninstall() -> anyhow::Result<PathBuf> {
    let exe = exe()?;
    fs::remove_file(&exe).with_context(|| format!("remove {}", exe.display()))?;
    Ok(exe)
}

/// Copies `new` beside `to` and renames it over `to`.
/// A rename within one directory is atomic, and a running binary keeps its
/// old inode, so a process that replaces itself is unaffected.
#[cfg(unix)]
pub(crate) fn place(new: &Path, to: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let staged = to.with_file_name(format!(".synth-{}.tmp", std::process::id()));
    fs::copy(new, &staged).with_context(|| format!("write {}", staged.display()))?;
    let result = fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
        .and_then(|()| fs::rename(&staged, to));
    if let Err(e) = result {
        let _ = fs::remove_file(&staged);
        return Err(e).with_context(|| format!("replace {}", to.display()));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn place(_new: &Path, _to: &Path) -> anyhow::Result<()> {
    bail!("cannot replace a running binary on this platform yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        bin_dir(xdg.map(OsString::from), home.map(OsString::from))
    }

    #[test]
    fn bin_dir_prefers_xdg() {
        assert_eq!(dir(Some("/x/bin"), Some("/h")), Some("/x/bin".into()));
        assert_eq!(dir(None, Some("/h")), Some("/h/.local/bin".into()));
    }

    #[test]
    fn bin_dir_ignores_relative_paths() {
        assert_eq!(dir(Some("bin"), Some("/h")), Some("/h/.local/bin".into()));
        assert_eq!(dir(Some(""), Some("/h")), Some("/h/.local/bin".into()));
        assert_eq!(dir(None, Some("h")), None);
        assert_eq!(dir(None, None), None);
    }
}
