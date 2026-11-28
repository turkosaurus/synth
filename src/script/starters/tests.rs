use super::*;

fn custom(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("synth-starters-{test}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn every_kind_has_a_starter() {
    let kinds: Vec<&str> = STARTERS.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(kinds, super::super::PATCH_KINDS);
}

#[test]
fn an_empty_folder_gets_its_starter_once() {
    let dir = custom("once");
    assert_eq!(write(&dir).unwrap().len(), STARTERS.len());
    assert!(dir.join("patterns").join(FILE).exists());
    assert!(write(&dir).unwrap().is_empty(), "already there");
}

#[test]
fn a_folder_with_a_patch_of_its_own_gets_none() {
    let dir = custom("own");
    fs::create_dir_all(dir.join("instruments")).unwrap();
    fs::write(dir.join("instruments").join("bass.lua"), "return {}").unwrap();
    let wrote = write(&dir).unwrap();
    assert!(!wrote.contains(&"instruments/example.lua".to_string()));
    assert!(!dir.join("instruments").join(FILE).exists());
    // a deleted starter stays deleted while the folder has other patches
    fs::remove_file(dir.join("tunings").join(FILE)).unwrap();
    fs::write(dir.join("tunings").join("just.lua"), "return {}").unwrap();
    assert!(write(&dir).unwrap().is_empty());
}
