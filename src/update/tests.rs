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

/// A gzipped tar holding each `(path, contents)`, as the release workflow
/// packs it with `tar -czf … -C pkg .`.
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(gz);
    for (path, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, path, *data).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

#[test]
fn unpack_reads_the_root_binary() {
    // the platform's own name: synth, or synth.exe on Windows
    let nested = format!("./nested/{}", install::FILE);
    let root = format!("./{}", install::FILE);
    let files: [(&str, &[u8]); 3] = [
        ("./LICENSE", b"gpl"),
        (&nested, b"wrong"),
        (&root, b"binary"),
    ];
    assert_eq!(unpack(&archive(&files)).unwrap(), b"binary");
    assert_eq!(
        unpack(&archive(&[(install::FILE, b"bare")])).unwrap(),
        b"bare"
    );
    assert!(unpack(&archive(&[("./LICENSE", b"gpl")])).is_err());
    assert!(unpack(b"not gzip").is_err());
}

#[test]
fn nothing_to_pick() {
    assert_eq!(picked(&["v0.3.4"], Channel::Dev(None)), None);
    assert_eq!(picked(&[], Channel::Stable), None);
}
