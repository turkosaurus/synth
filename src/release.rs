//! The build's identity, as `--version` prints it.

/// `Cargo.toml`'s version, or the release workflow's `SYNTH_VERSION`.
/// A dev build sets it to the version plus semver build metadata,
/// as in `0.3.4+beef3d0`, so the binary names the tag it ships under.
/// The name is a literal because `option_env!` accepts nothing else.
pub const VERSION: &str = match option_env!("SYNTH_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
