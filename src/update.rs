//! `synth update`: install a release channel's build from GitHub releases.
//!
//! Every push to `main` publishes a release, and its tag names the channel:
//! `v0.4.0` is stable, `v0.4.0-beta.1` is beta,
//! and `v0.3.4+beef3d0` is a dev build of commit `beef3d0`.
//! HTTPS, gzip, and tar are all in the binary,
//! so `update` needs nothing on the machine and a static build stays static.

use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{bail, ensure, Context};
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::install;
use crate::release::VERSION;

const RELEASES: &str = "https://api.github.com/repos/turkosaurus/synth/releases";
const ACCEPT_JSON: &str = "application/vnd.github+json";

/// A token lifts the API's limit from 60 requests an hour per IP to 5,000.
/// The variables are checked in `gh`'s own order, then `gh auth token` asked.
/// It is sent to the API only, never with a download.
const TOKEN_VARS: [&str; 2] = ["GH_TOKEN", "GITHUB_TOKEN"];
const GH: &str = "gh";
const UNAUTHORIZED: u16 = 401;
const USER_AGENT: &str = concat!("synth/", env!("CARGO_PKG_VERSION"));

/// GitHub serves at most 100 releases a page.
/// Ten pages bounds the requests until old dev builds are pruned.
const PER_PAGE: usize = 100;
const PAGES: usize = 10;

/// Network errors and 5xx responses are retried, waiting 1s then 2s.
const ATTEMPTS: u32 = 3;
const BACKOFF: Duration = Duration::from_secs(1);
const TIMEOUT: Duration = Duration::from_secs(60);
/// Release archives are a few MB; this only stops a runaway response.
const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Tags are `v` plus semver; `+` starts build metadata, `-` a prerelease.
const TAG_PREFIX: char = 'v';
const DEV_MARK: char = '+';
const BETA_MARK: char = '-';
/// A dev tag carries this many characters of its commit.
const SHA_LEN: usize = 7;
/// `--dev main` names the newest dev build.
pub const DEV_NEWEST: &str = "main";

/// GitHub's asset digest is `sha256:<hex>`.
const DIGEST_PREFIX: &str = "sha256:";

/// Release assets are `synth-<target>.tar.gz`, holding the binary at the root.
const ASSET_PREFIX: &str = "synth-";
const ASSET_SUFFIX: &str = ".tar.gz";

/// The target this binary was built for, if the release workflow builds one.
const TARGET: Option<&str> = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("x86_64-unknown-linux-gnu")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("aarch64-apple-darwin")
} else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("x86_64-pc-windows-msvc")
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
    /// The binary at `path` was replaced.
    Installed {
        from: String,
        to: String,
        path: PathBuf,
    },
    /// `--dry-run`: the build was downloaded, verified, and unpacked,
    /// and would replace `path`.
    Checked { to: String, path: PathBuf },
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
    let version = tag.strip_prefix(TAG_PREFIX)?;
    if version.contains(DEV_MARK) {
        Some(Kind::Dev)
    } else if version.contains(BETA_MARK) {
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
            // a longer sha is cut to the tag's length to match
            let prefix = sha.get(..SHA_LEN).unwrap_or(sha);
            releases.iter().find(|r| {
                kind(&r.tag_name) == Some(Kind::Dev)
                    && r.tag_name
                        .split_once(DEV_MARK)
                        .is_some_and(|(_, meta)| meta.starts_with(prefix))
            })
        }
    }
}

/// Installs the channel's build over the running binary,
/// unless it is the installed one already.
/// A dry run downloads, verifies, and unpacks it even then, and stops there.
pub fn run(channel: &Channel, dry_run: bool) -> anyhow::Result<Outcome> {
    let Some(target) = TARGET else {
        bail!("no release is built for this platform");
    };
    let asset_name = format!("{ASSET_PREFIX}{target}{ASSET_SUFFIX}");

    let releases = releases()?;
    let Some(release) = pick(&releases, channel) else {
        match channel {
            Channel::Stable => bail!("no stable release"),
            Channel::Beta => bail!("no beta newer than stable, and no dev build"),
            Channel::Dev(None) => bail!("no dev build"),
            Channel::Dev(Some(sha)) => bail!("no dev build of {sha}"),
        }
    };
    let to = release.tag_name.trim_start_matches(TAG_PREFIX).to_string();
    if to == VERSION && !dry_run {
        return Ok(Outcome::Current(to));
    }

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| format!("{} has no {asset_name}", release.tag_name))?;
    let exe = install::exe()?;
    let archive = get(&asset.browser_download_url, &[])
        .with_context(|| format!("download {}", asset.name))?;
    verify(asset, &archive)?;
    let binary = unpack(&archive).with_context(|| format!("unpack {}", asset.name))?;

    if dry_run {
        return Ok(Outcome::Checked { to, path: exe });
    }
    install::place(&binary, &exe)?;
    Ok(Outcome::Installed {
        from: VERSION.to_string(),
        to,
        path: exe,
    })
}

/// A GitHub token from the environment or `gh`, if there is one.
fn token() -> Option<String> {
    let from_env = TOKEN_VARS.iter().find_map(|var| env::var(var).ok());
    let token = match from_env {
        Some(token) => token,
        None => {
            // gh may be missing or logged out; either way there is no token
            let out = Command::new(GH).args(["auth", "token"]).output().ok()?;
            if !out.status.success() {
                return None;
            }
            String::from_utf8(out.stdout).ok()?
        }
    };
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// Every published release, newest first.
/// Without a token, or with one GitHub rejects, it warns and asks unauthenticated.
fn releases() -> anyhow::Result<Vec<Release>> {
    let mut auth = token().map(|t| format!("Bearer {t}"));
    if auth.is_none() {
        eprintln!(
            "warning: no GitHub token in {} or from `{GH} auth token`; \
             unauthenticated requests are limited to 60 an hour",
            TOKEN_VARS.join(" or ")
        );
    }
    let mut all = Vec::new();
    for page in 1..=PAGES {
        let url = format!("{RELEASES}?per_page={PER_PAGE}&page={page}");
        let mut headers = vec![("Accept", ACCEPT_JSON)];
        if let Some(auth) = &auth {
            headers.push(("Authorization", auth));
        }
        let body = match get(&url, &headers) {
            Err(e) if auth.is_some() && status(&e) == Some(UNAUTHORIZED) => {
                eprintln!("warning: GitHub rejected the token; retrying unauthenticated");
                auth = None;
                get(&url, &[("Accept", ACCEPT_JSON)])
            }
            body => body,
        }
        .context("list releases")?;
        let releases: Vec<Release> =
            serde_json::from_slice(&body).context("read the release list")?;
        let last = releases.len() < PER_PAGE;
        all.extend(releases.into_iter().filter(|r| !r.draft));
        if last {
            break;
        }
    }
    // ISO 8601 timestamps sort as strings
    all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(all)
}

/// The HTTP status an error from `get` carries, if it is one.
fn status(e: &anyhow::Error) -> Option<u16> {
    match e.downcast_ref::<ureq::Error>() {
        Some(ureq::Error::StatusCode(code)) => Some(*code),
        _ => None,
    }
}

/// GETs `url` with `headers` over HTTPS only, redirects included,
/// retrying network errors and 5xx responses.
fn get(url: &str, headers: &[(&str, &str)]) -> anyhow::Result<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .user_agent(USER_AGENT)
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let mut wait = BACKOFF;
    let mut attempt = 1;
    loop {
        let mut request = agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        match request.call() {
            Ok(mut response) => {
                return response
                    .body_mut()
                    .with_config()
                    .limit(MAX_BYTES)
                    .read_to_vec()
                    .with_context(|| format!("read {url}"));
            }
            Err(e) if attempt < ATTEMPTS && retryable(&e) => {
                thread::sleep(wait);
                wait *= 2;
                attempt += 1;
            }
            Err(e) => return Err(e).with_context(|| format!("GET {url}")),
        }
    }
}

/// Errors a second try can fix: the network, and the server's own faults.
/// A 4xx, TLS, or limit error fails the same way every time.
fn retryable(e: &ureq::Error) -> bool {
    match e {
        ureq::Error::StatusCode(code) => *code >= 500,
        ureq::Error::Io(_)
        | ureq::Error::Timeout(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed => true,
        _ => false,
    }
}

/// Checks the download against the digest GitHub recorded on upload.
fn verify(asset: &Asset, bytes: &[u8]) -> anyhow::Result<()> {
    let want = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix(DIGEST_PREFIX))
        .with_context(|| format!("{} has no sha256 digest", asset.name))?;
    let got = format!("{:x}", Sha256::digest(bytes));
    ensure!(
        got == want,
        "{} sha256 is {got}, expected {want}",
        asset.name
    );
    Ok(())
}

/// The binary at the root of a release archive.
/// Release archives hold it as `./synth`, or `./synth.exe` for Windows; only
/// that entry is read.
fn unpack(archive: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut tar = tar::Archive::new(GzDecoder::new(archive));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let at_root = path
            .parent()
            .is_some_and(|p| p == Path::new("") || p == Path::new("."));
        if at_root && path.file_name().is_some_and(|n| n == install::FILE) {
            let mut binary = Vec::new();
            entry.read_to_end(&mut binary)?;
            return Ok(binary);
        }
    }
    bail!("no {} in the archive", install::FILE)
}

#[cfg(test)]
mod tests;
