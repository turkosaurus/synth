//! `synth install` and `synth uninstall`: put the running binary on PATH,
//! or take it off. `update` swaps binaries with the same `place`.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};

/// The binary's name, without the platform's executable suffix.
pub(crate) const BIN: &str = "synth";

/// The binary's file name, installed and inside release archives.
#[cfg(windows)]
pub(crate) const FILE: &str = "synth.exe";
#[cfg(not(windows))]
pub(crate) const FILE: &str = "synth";

const XDG_BIN_HOME: &str = "XDG_BIN_HOME";
const HOME: &str = "HOME";
/// Windows' home, used when `HOME` is not set
const USERPROFILE: &str = "USERPROFILE";
const PATH: &str = "PATH";
/// The XDG spec's user bin directory, under `$HOME`.
const LOCAL_BIN: [&str; 2] = [".local", "bin"];

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

/// `$XDG_BIN_HOME`, else `~/.local/bin`, where `~` is `HOME`, or on
/// Windows, which does not set `HOME`, `USERPROFILE`.
/// The XDG spec names only `~/.local/bin`; `XDG_BIN_HOME` is not in it,
/// but installers such as uv honor it, uv uses the same folder on Windows,
/// and the spec ignores relative paths.
fn bin_dir(
    xdg: Option<OsString>,
    home: Option<OsString>,
    userprofile: Option<OsString>,
    windows: bool,
) -> Option<PathBuf> {
    let absolute = |value: Option<OsString>| value.map(PathBuf::from).filter(|p| p.is_absolute());
    absolute(xdg).or_else(|| {
        let home = absolute(home).or_else(|| windows.then(|| absolute(userprofile)).flatten())?;
        Some(LOCAL_BIN.iter().fold(home, |dir, part| dir.join(part)))
    })
}

/// Copies the running binary into the user's bin directory.
pub fn install() -> anyhow::Result<Installed> {
    let exe = exe()?;
    let Some(dir) = bin_dir(
        env::var_os(XDG_BIN_HOME),
        env::var_os(HOME),
        env::var_os(USERPROFILE),
        cfg!(windows),
    ) else {
        bail!("none of {XDG_BIN_HOME}, {HOME} or {USERPROFILE} is an absolute path");
    };
    let to = dir.join(FILE);
    if fs::canonicalize(&to).is_ok_and(|installed| installed == exe) {
        return Ok(Installed::Already(to));
    }
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let binary = fs::read(&exe).with_context(|| format!("read {}", exe.display()))?;
    place(&binary, &to)?;

    let on_path = env::var_os(PATH).is_some_and(|path| env::split_paths(&path).any(|d| d == dir));
    Ok(Installed::Copied { to, on_path })
}

/// Removes the running binary, wherever it was installed.
/// synth keeps no other files yet; when it does, they go here too.
pub fn uninstall() -> anyhow::Result<PathBuf> {
    let exe = exe()?;
    remove_running(&exe).with_context(|| format!("remove {}", exe.display()))?;
    Ok(exe)
}

/// Unix deletes a running binary; the process keeps its inode until exit.
#[cfg(not(windows))]
fn remove_running(exe: &Path) -> std::io::Result<()> {
    fs::remove_file(exe)
}

/// Windows refuses to delete a running `.exe` but lets it be renamed. It
/// moves into the temp folder, which Windows cleans, or, when that is on
/// another drive a rename cannot cross, to `synth.exe.old` beside it, which
/// the next install or update removes.
#[cfg(windows)]
fn remove_running(exe: &Path) -> std::io::Result<()> {
    let temp = env::temp_dir().join(format!("{BIN}-{}.exe.old", std::process::id()));
    fs::rename(exe, &temp).or_else(|_| fs::rename(exe, exe.with_extension("exe.old")))
}

/// Writes `binary` beside `to` and renames it over `to`.
/// A rename within one directory is atomic. On Unix a running binary keeps
/// its old inode, so a process that replaces itself is unaffected.
pub(crate) fn place(binary: &[u8], to: &Path) -> anyhow::Result<()> {
    let staged = to.with_file_name(format!(".{BIN}-{}.tmp", std::process::id()));
    fs::write(&staged, binary).with_context(|| format!("write {}", staged.display()))?;
    let result = executable(&staged)
        .and_then(|()| set_aside(to))
        .and_then(|()| fs::rename(&staged, to));
    if let Err(e) = result {
        let _ = fs::remove_file(&staged);
        return Err(e).with_context(|| format!("replace {}", to.display()));
    }
    Ok(())
}

#[cfg(unix)]
fn executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

/// Windows has no executable bit: the `.exe` name is what makes it run.
#[cfg(not(unix))]
fn executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Nothing to do on Unix, where a rename replaces a running binary.
#[cfg(not(windows))]
fn set_aside(_to: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Windows refuses to replace a running `.exe` but lets it be renamed, so
/// the old one moves to `synth.exe.old` first. Old copies left by earlier
/// runs, beside it or in the temp folder, are removed here once nothing
/// runs from them; one still running stays until a later run.
#[cfg(windows)]
fn set_aside(to: &Path) -> std::io::Result<()> {
    let old = to.with_extension("exe.old");
    let _ = fs::remove_file(&old);
    if let Ok(entries) = fs::read_dir(env::temp_dir()) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&format!("{BIN}-")) && name.ends_with(".exe.old") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    match fs::rename(to, &old) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
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

    fn dir(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        bin_dir(xdg.map(var), home.map(var), None, false)
    }

    #[test]
    fn windows_falls_back_to_userprofile() {
        let profile = Some(var("/users/me"));
        assert_eq!(
            bin_dir(None, None, profile.clone(), true),
            Some(abs("/users/me/.local/bin"))
        );
        assert_eq!(
            bin_dir(None, Some(var("/h")), profile.clone(), true),
            Some(abs("/h/.local/bin")),
            "HOME still wins when set"
        );
        assert_eq!(bin_dir(None, None, profile, false), None, "Unix ignores it");
    }

    #[test]
    fn place_replaces_an_existing_binary() {
        let dir = std::env::temp_dir().join(format!("synth-place-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let to = dir.join(FILE);
        fs::write(&to, b"old").unwrap();
        place(b"new", &to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"new");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bin_dir_prefers_xdg() {
        assert_eq!(dir(Some("/x/bin"), Some("/h")), Some(abs("/x/bin")));
        assert_eq!(dir(None, Some("/h")), Some(abs("/h/.local/bin")));
    }

    #[test]
    fn bin_dir_ignores_relative_paths() {
        assert_eq!(dir(Some("bin"), Some("/h")), Some(abs("/h/.local/bin")));
        assert_eq!(dir(Some(""), Some("/h")), Some(abs("/h/.local/bin")));
        assert_eq!(dir(None, Some("h")), None);
        assert_eq!(dir(None, None), None);
    }
}
