//! `synth update`: install a release channel's build from GitHub releases.
//!
//! Every push to `main` publishes one release, and its tag names the channel:
//! `v0.4.0` is stable, `v0.4.0-beta.1` is beta,
//! and `v0.3.4+beef3d0` is a dev build of commit `beef3d0`.
//! `curl` and `tar` do the network and the archive, as they ship with every
//! platform synth releases for, so the binary carries no TLS stack.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, ensure, Context};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::install;
use crate::release::VERSION;

const RELEASES: &str = "https://api.github.com/repos/turkosaurus/synth/releases";

/// GitHub serves at most 100 releases a page.
/// Ten pages bounds the requests until old dev builds are pruned.
const PAGES: usize = 10;

/// The release asset built for this platform, if the release workflow
/// builds one and `update` can replace a running binary on it.
const ASSET: Option<&str> = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("synth-x86_64-unknown-linux-gnu.tar.gz")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("synth-aarch64-apple-darwin.tar.gz")
} else {
    None
};

/// Which builds `update` follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// The latest release.
    Stable,
    /// The latest beta while it is newer than stable, else dev.
    Beta,
    /// The newest dev build, or the one built from a commit.
    Dev(Option<String>),
}

/// What `update` did.
#[derive(Debug)]
pub enum Outcome {
    /// The installed build is already the channel's.
    Current(String),
    /// The binary was replaced.
    Installed { from: String, to: String },
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    created_at: String,
    #[serde(default)]
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`, computed by GitHub on upload.
    digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Stable,
    Beta,
    Dev,
}

/// The channel a tag belongs to, read from semver's own markers:
/// build metadata (`+`) is dev, a prerelease (`-`) is beta.
fn kind(tag: &str) -> Option<Kind> {
    let version = tag.strip_prefix('v')?;
    if version.contains('+') {
        Some(Kind::Dev)
    } else if version.contains('-') {
        Some(Kind::Beta)
    } else {
        Some(Kind::Stable)
    }
}

/// Picks the release a channel installs from releases sorted newest first.
/// The version check only lets versions move forward,
/// so the newest stable or beta is also the highest.
fn pick<'a>(releases: &'a [Release], channel: &Channel) -> Option<&'a Release> {
    let of = |want: Kind| {
        releases
            .iter()
            .find(move |r| kind(&r.tag_name) == Some(want))
    };
    match channel {
        Channel::Stable => of(Kind::Stable),
        Channel::Beta => {
            // a beta newer than every stable is the newest non-dev release
            let newest = releases
                .iter()
                .find(|r| matches!(kind(&r.tag_name), Some(Kind::Stable | Kind::Beta)))?;
            match kind(&newest.tag_name) {
                Some(Kind::Beta) => Some(newest),
                _ => of(Kind::Dev),
            }
        }
        Channel::Dev(None) => of(Kind::Dev),
        Channel::Dev(Some(sha)) => {
            // tags carry 7 characters, so a longer sha is cut to match
            let prefix = sha.get(..7).unwrap_or(sha);
            releases.iter().find(|r| {
                kind(&r.tag_name) == Some(Kind::Dev)
                    && r.tag_name
                        .split_once('+')
                        .is_some_and(|(_, meta)| meta.starts_with(prefix))
            })
        }
    }
}

/// Installs the channel's build over the running binary,
/// unless it is the installed one already.
pub fn run(channel: &Channel) -> anyhow::Result<Outcome> {
    if cfg!(windows) {
        bail!("update cannot replace a running binary on Windows yet");
    }
    let Some(asset_name) = ASSET else {
        bail!("no release is built for this platform");
    };

    let releases = releases()?;
    let Some(release) = pick(&releases, channel) else {
        match channel {
            Channel::Stable => bail!("no stable release"),
            Channel::Beta => bail!("no beta newer than stable, and no dev build"),
            Channel::Dev(None) => bail!("no dev build"),
            Channel::Dev(Some(sha)) => bail!("no dev build of {sha}"),
        }
    };
    let to = release.tag_name.trim_start_matches('v').to_string();
    if to == VERSION {
        return Ok(Outcome::Current(to));
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| format!("{} has no {asset_name}", release.tag_name))?;
    let exe = install::exe()?;
    let dir = scratch()?;
    let result = fetch(asset, &exe, &dir);
    // best effort: a leftover temp directory is not worth failing for
    let _ = fs::remove_dir_all(&dir);
    result?;

    Ok(Outcome::Installed {
        from: VERSION.to_string(),
        to,
    })
}

/// Every published release, newest first.
fn releases() -> anyhow::Result<Vec<Release>> {
    let mut all = Vec::new();
    for page in 1..=PAGES {
        let url = format!("{RELEASES}?per_page=100&page={page}");
        let body =
            curl(&["-H", "Accept: application/vnd.github+json", &url]).context("list releases")?;
        let releases: Vec<Release> =
            serde_json::from_slice(&body).context("read the release list")?;
        let last = releases.len() < 100;
        all.extend(releases.into_iter().filter(|r| !r.draft));
        if last {
            break;
        }
    }
    // ISO 8601 timestamps sort as strings
    all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(all)
}

/// Downloads, verifies, and unpacks the asset, then swaps it in for `exe`.
fn fetch(asset: &Asset, exe: &Path, dir: &Path) -> anyhow::Result<()> {
    let archive = dir.join(&asset.name);
    let archive_str = archive.to_str().context("temp path is not UTF-8")?;
    curl(&["-o", archive_str, &asset.browser_download_url])
        .with_context(|| format!("download {}", asset.name))?;

    let bytes = fs::read(&archive).with_context(|| format!("read {}", archive.display()))?;
    verify(asset, &bytes)?;

    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(dir)
        .status()
        .context("run tar")?;
    ensure!(status.success(), "tar could not unpack {}", asset.name);

    install::place(&dir.join("synth"), exe)
}

/// Checks the download against the digest GitHub recorded on upload.
fn verify(asset: &Asset, bytes: &[u8]) -> anyhow::Result<()> {
    let want = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .with_context(|| format!("{} has no sha256 digest", asset.name))?;
    let got = format!("{:x}", Sha256::digest(bytes));
    ensure!(
        got == want,
        "{} sha256 is {got}, expected {want}",
        asset.name
    );
    Ok(())
}

/// A fresh directory under the system temp directory.
fn scratch() -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("synth-update-{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("clear {}", dir.display()))?;
    }
    fs::create_dir(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// Runs curl over HTTPS only, redirects included, and returns stdout.
fn curl(args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let out = Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--proto-redir", "=https"])
        .args(args)
        .output()
        .context("run curl")?;
    if !out.status.success() {
        bail!("curl: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Releases newest first, as `releases` returns them.
    fn list(tags: &[&str]) -> Vec<Release> {
        tags.iter()
            .enumerate()
            .map(|(i, tag)| Release {
                tag_name: (*tag).to_string(),
                created_at: format!("2026-11-{:02}T00:00:00Z", 30 - i),
                draft: false,
                assets: Vec::new(),
            })
            .collect()
    }

    fn picked(tags: &[&str], channel: Channel) -> Option<String> {
        pick(&list(tags), &channel).map(|r| r.tag_name.clone())
    }

    #[test]
    fn kinds() {
        assert_eq!(kind("v0.4.0"), Some(Kind::Stable));
        assert_eq!(kind("v0.4.0-beta.1"), Some(Kind::Beta));
        assert_eq!(kind("v0.3.4+beef3d0"), Some(Kind::Dev));
        assert_eq!(kind("v0.4.0-beta.1+beef3d0"), Some(Kind::Dev));
        assert_eq!(kind("nightly"), None);
    }

    #[test]
    fn stable_skips_prereleases() {
        let tags = ["v0.4.0-beta.1+aaaaaaa", "v0.4.0-beta.1", "v0.3.4"];
        assert_eq!(picked(&tags, Channel::Stable).as_deref(), Some("v0.3.4"));
    }

    #[test]
    fn beta_newer_than_stable() {
        let tags = ["v0.4.0-beta.1+aaaaaaa", "v0.4.0-beta.1", "v0.3.4"];
        assert_eq!(
            picked(&tags, Channel::Beta).as_deref(),
            Some("v0.4.0-beta.1")
        );
    }

    #[test]
    fn beta_falls_back_to_dev() {
        let tags = ["v0.4.0+bbbbbbb", "v0.4.0", "v0.4.0-beta.1", "v0.3.4"];
        assert_eq!(
            picked(&tags, Channel::Beta).as_deref(),
            Some("v0.4.0+bbbbbbb")
        );
    }

    #[test]
    fn dev_newest_or_by_sha() {
        let tags = ["v0.3.4+bbbbbbb", "v0.3.4+aaaaaaa", "v0.3.4"];
        assert_eq!(
            picked(&tags, Channel::Dev(None)).as_deref(),
            Some("v0.3.4+bbbbbbb")
        );
        let sha = Some("aaaaaaa0123456789".to_string());
        assert_eq!(
            picked(&tags, Channel::Dev(sha)).as_deref(),
            Some("v0.3.4+aaaaaaa")
        );
        let sha = Some("ccc".to_string());
        assert_eq!(picked(&tags, Channel::Dev(sha)), None);
    }

    #[test]
    fn digest() {
        // `printf synth | sha256sum`
        let sum = "sha256:e8c3e4faa6b27058ee06d25eabe8643618863a992a216ed57d9f6a58021e01c4";
        let asset = |digest: Option<&str>| Asset {
            name: "a.tar.gz".to_string(),
            browser_download_url: String::new(),
            digest: digest.map(str::to_string),
        };
        assert!(verify(&asset(Some(sum)), b"synth").is_ok());
        assert!(verify(&asset(Some(sum)), b"synth!").is_err());
        assert!(verify(&asset(None), b"synth").is_err());
    }

    #[test]
    fn nothing_to_pick() {
        assert_eq!(picked(&["v0.3.4"], Channel::Dev(None)), None);
        assert_eq!(picked(&[], Channel::Stable), None);
    }
}
