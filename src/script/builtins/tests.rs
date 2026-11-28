use super::*;

/// An empty custom folder for one test.
fn custom(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synth-builtins-{test}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn wave(custom: &Path) -> PathBuf {
    custom.join(DIR).join("wave.lua")
}

fn outcome(wrote: &[(&str, Wrote)], name: &str) -> &'static str {
    match wrote.iter().find(|(n, _)| *n == name).map(|(_, w)| w) {
        Some(Wrote::Written) => "written",
        Some(Wrote::Refreshed) => "refreshed",
        Some(Wrote::Current) => "current",
        Some(Wrote::Kept) => "kept",
        Some(Wrote::KeptBehind) => "kept behind",
        None => "missing",
    }
}

/// Records `base` as wave.lua's base, as an older synth would have.
fn set_base(custom: &Path, base: &[u8]) {
    let path = custom.join(DIR).join(BASES);
    let text = fs::read_to_string(&path).unwrap();
    let text: String = text
        .lines()
        .map(|line| match line.ends_with("  wave.lua") {
            true => format!("{}  wave.lua\n", hash(base)),
            false => format!("{line}\n"),
        })
        .collect();
    fs::write(path, text).unwrap();
}

#[test]
fn the_first_write_copies_every_built_in_unedited() {
    let dir = custom("first");
    let wrote = write(&dir).unwrap();
    assert!(wrote.iter().all(|(_, w)| *w == Wrote::Written), "{wrote:?}");
    assert_eq!(wrote.len(), MODULES.len() + GUIDES.len());
    let record = fs::read_to_string(dir.join(DIR).join(BASES)).unwrap();
    assert_eq!(record.lines().count(), MODULES.len());
    let record = fs::read_to_string(dir.join(BASES)).unwrap();
    assert_eq!(record.lines().count(), GUIDES.len());
    assert!(dir.join("AGENTS.md").exists());
    assert!(states(&dir).iter().all(|(_, s)| *s == State::BuiltIn));
    assert!(write(&dir)
        .unwrap()
        .iter()
        .all(|(_, w)| *w == Wrote::Current));
}

#[test]
fn an_edited_copy_replaces_the_built_in_and_is_kept() {
    let dir = custom("edited");
    write(&dir).unwrap();
    fs::write(wave(&dir), "return { edited = true }").unwrap();

    assert!(edited(&dir, "synth.wave").is_some());
    assert_eq!(
        states(&dir)
            .into_iter()
            .find(|(n, _)| *n == "synth.wave")
            .unwrap()
            .1,
        State::Edited(wave(&dir))
    );
    assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "kept");
    assert_eq!(
        fs::read_to_string(wave(&dir)).unwrap(),
        "return { edited = true }"
    );
}

#[test]
fn an_unedited_copy_follows_a_newer_built_in() {
    let dir = custom("refresh");
    write(&dir).unwrap();
    // as if an older synth wrote this copy, unedited since
    fs::write(wave(&dir), "old").unwrap();
    set_base(&dir, b"old");

    assert!(
        edited(&dir, "synth.wave").is_none(),
        "unedited, so not used"
    );
    assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "refreshed");
    let built_in = MODULES.iter().find(|(n, _)| *n == "synth.wave").unwrap().1;
    assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), built_in);
}

#[test]
fn an_edited_copy_behind_a_newer_built_in_is_kept_and_named() {
    let dir = custom("behind");
    write(&dir).unwrap();
    fs::write(wave(&dir), "mine").unwrap();
    set_base(&dir, b"old");

    assert_eq!(
        states(&dir)
            .into_iter()
            .find(|(n, _)| *n == "synth.wave")
            .unwrap()
            .1,
        State::EditedBehind(wave(&dir))
    );
    assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "kept behind");
    assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), "mine");
    // still behind, so the next run still knows
    assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "kept behind");
}

#[test]
fn overwrite_takes_the_new_built_in_and_keep_stops_asking() {
    let dir = custom("resolve");
    write(&dir).unwrap();
    fs::write(wave(&dir), "mine").unwrap();
    set_base(&dir, b"old");

    let stale = behind(&dir, "synth.wave").unwrap().expect("behind");
    assert_eq!(stale.copy, "mine");
    keep(&dir, &stale).unwrap();
    assert!(
        behind(&dir, "synth.wave").unwrap().is_none(),
        "kept, so not asked again"
    );
    assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), "mine");
    assert!(
        edited(&dir, "synth.wave").is_some(),
        "still the user's edit"
    );

    set_base(&dir, b"old");
    let stale = behind(&dir, "synth.wave").unwrap().expect("behind again");
    overwrite(&dir, &stale).unwrap();
    assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), stale.built_in);
    assert!(edited(&dir, "synth.wave").is_none(), "now unedited");
}

#[test]
fn a_copy_with_no_record_is_kept() {
    let dir = custom("unrecorded");
    fs::create_dir_all(dir.join(DIR)).unwrap();
    fs::write(wave(&dir), "someone's own").unwrap();

    assert_eq!(outcome(&write(&dir).unwrap(), "synth.wave"), "kept");
    assert_eq!(fs::read_to_string(wave(&dir)).unwrap(), "someone's own");
}

#[test]
fn an_edited_guide_is_kept_and_an_unedited_one_follows() {
    let dir = custom("guides");
    write(&dir).unwrap();
    fs::write(dir.join("AGENTS.md"), "mine").unwrap();
    fs::write(dir.join("README.md"), "old").unwrap();
    let record = fs::read_to_string(dir.join(BASES)).unwrap();
    let record: String = record
        .lines()
        .map(|line| match line.ends_with("  README.md") {
            true => format!("{}  README.md\n", hash(b"old")),
            false => format!("{line}\n"),
        })
        .collect();
    fs::write(dir.join(BASES), record).unwrap();

    let wrote = write(&dir).unwrap();
    assert_eq!(outcome(&wrote, "AGENTS.md"), "kept");
    assert_eq!(outcome(&wrote, "README.md"), "refreshed");
    assert_eq!(fs::read_to_string(dir.join("AGENTS.md")).unwrap(), "mine");
    assert_eq!(
        fs::read_to_string(dir.join("README.md")).unwrap(),
        GUIDES[0].1
    );
}

#[test]
fn the_root_module_is_init_lua() {
    assert_eq!(file_name("synth"), "init.lua");
    assert_eq!(file_name("synth.input"), "input.lua");
    assert_eq!(file_name("synth.input.keyboard"), "input/keyboard.lua");
}
