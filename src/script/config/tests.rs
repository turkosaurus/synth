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

/// A Unix environment with these XDG and HOME values.
fn unix(xdg: Option<&str>, home: Option<&str>) -> Dirs {
    Dirs {
        xdg_config: xdg.map(var),
        xdg_data: xdg.map(var),
        xdg_state: xdg.map(var),
        xdg_runtime: xdg.map(var),
        home: home.map(var),
        appdata: None,
        userprofile: None,
        windows: false,
    }
}

/// A Windows environment with these APPDATA and USERPROFILE values.
fn windows(appdata: Option<&str>, userprofile: Option<&str>) -> Dirs {
    Dirs {
        xdg_config: None,
        xdg_data: None,
        xdg_state: None,
        xdg_runtime: None,
        home: None,
        appdata: appdata.map(var),
        userprofile: userprofile.map(var),
        windows: true,
    }
}

fn at(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    unix(xdg, home).config().map(|dir| under(dir, &FILE))
}

#[test]
fn the_path_prefers_xdg() {
    assert_eq!(at(Some("/x"), Some("/h")), Some(abs("/x/synth/config.lua")));
    assert_eq!(
        at(None, Some("/h")),
        Some(abs("/h/.config/synth/config.lua"))
    );
}

#[test]
fn the_path_ignores_relative_dirs() {
    assert_eq!(
        at(Some("x"), Some("/h")),
        Some(abs("/h/.config/synth/config.lua"))
    );
    assert_eq!(at(None, Some("h")), None);
    assert_eq!(at(None, None), None);
}

#[test]
fn the_log_prefers_xdg_state() {
    let log = |xdg, home| unix(xdg, home).state().map(|dir| under(dir, &LOG));
    assert_eq!(log(Some("/x"), Some("/h")), Some(abs("/x/synth/log.jsonl")));
    assert_eq!(
        log(None, Some("/h")),
        Some(abs("/h/.local/state/synth/log.jsonl"))
    );
    assert_eq!(log(Some("x"), None), None);
}

#[test]
fn the_socket_prefers_xdg_runtime_then_state() {
    let socket = |dirs: Dirs| dirs.runtime().map(|dir| under(dir, &SOCKET));
    assert_eq!(
        socket(unix(Some("/x"), Some("/h"))),
        Some(abs("/x/synth/log.sock"))
    );
    assert_eq!(
        socket(unix(None, Some("/h"))),
        Some(abs("/h/.local/state/synth/log.sock"))
    );
    assert_eq!(socket(windows(Some("/appdata"), None)), None);
}

fn user(source: &str) -> Result<Config, Error> {
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
fn a_config_runs_in_the_sandbox() {
    let config = resolve(Some(("return { queue = os and 32 or 64 }", "test"))).unwrap();
    assert_eq!(config.queue, 64);
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
    assert_eq!(config.tui.latency_window, Duration::from_secs(5));
    assert_eq!(config.tui.latency_records, 1024);
    assert_eq!(config.tui.faults, 64);
    assert_eq!(config.tui.source_stream, StreamName::Output);
    assert_eq!(config.tui.source_channel, 0);
    assert_eq!(config.tui.source_tee, Duration::from_millis(100));
    assert_eq!(config.tui.scope_history, Duration::from_millis(250));
    assert_eq!(config.tui.scope_cycles, 2.0);
    assert_eq!(config.tui.spectrum_size, 4096);
    assert_eq!(config.tui.spectrum_floor_db, -60.0);
    assert_eq!(config.tui.spectrum_height, 50.0);
    assert_eq!(config.log.trim_size_kb, 1024);
    assert_eq!(config.tui.latency_floor_ms, 0.01);
    assert_eq!(config.tui.latency_ceil_ms, 100.0);
    let budget = |warn, danger| Budget { warn, danger };
    assert_eq!(config.tui.input_budget, budget(1.0, 5.0));
    assert_eq!(config.tui.render_budget, budget(5.0, 10.0));
    assert_eq!(config.tui.output_budget, budget(10.0, 20.0));
    assert_eq!(config.tui.total_budget, budget(10.0, 20.0));
    assert_eq!(config.tui.fps, budget(30.0, 24.0));
}

#[test]
fn an_empty_file_is_the_defaults() {
    let config = user("return {}").unwrap();
    assert_eq!(
        Config {
            outdated: false,
            ..config
        },
        resolve(None).unwrap()
    );
}

/// The built-in file's `version`.
fn built_in() -> Version {
    let lua = mlua::Lua::new();
    let base = table(&lua, TEMPLATE, TEMPLATE_NAME).unwrap();
    version(&base, TEMPLATE_NAME)
        .unwrap()
        .expect("config.lua names a version")
}

#[test]
fn versions_parse_and_order_as_semver() {
    let v = |text| Version::parse(text).unwrap_or_else(|| panic!("{text}"));
    assert!(v("0.17.1") < v("0.18.0-beta.1"));
    assert!(v("0.18.0-beta.1") < v("0.18.0-beta.2"));
    assert!(v("0.18.0-beta.2") < v("0.18.0"));
    assert!(v("0.9.0") < v("0.10.0"));
    assert_eq!(v("1.2.3-beta.4").to_string(), "1.2.3-beta.4");
    for bad in [
        "",
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.2.x",
        "1.2.3-rc.1",
        "v1.2.3",
    ] {
        assert_eq!(Version::parse(bad), None, "{bad}");
    }
}

#[test]
fn a_file_without_a_version_is_out_of_date() {
    assert!(user("return {}").unwrap().outdated);
}

#[test]
fn a_file_older_than_the_built_in_is_out_of_date() {
    let config = user("return { version = '0.0.1' }").unwrap();
    assert!(config.outdated);
}

#[test]
fn a_file_as_new_or_newer_is_current() {
    let current = format!("return {{ version = '{}' }}", built_in());
    assert!(!user(&current).unwrap().outdated);
    assert!(!user("return { version = '999.0.0' }").unwrap().outdated);
    assert!(!resolve(None).unwrap().outdated);
}

#[test]
fn a_bad_version_is_named() {
    let error = user("return { version = '1.2' }").unwrap_err();
    assert!(
        matches!(&error, Error::ConfigVersion { name, version } if name == "test" && version == "1.2"),
        "{error}"
    );
    let error = user("return { version = 1 }").unwrap_err();
    assert!(
        format!("{error:#}").contains("`version` must be a string"),
        "{error:#}"
    );
}

fn rebuilt(source: &str) -> String {
    rebuild_for(source, "test", &unix(Some("/d"), None))
        .unwrap_or_else(|e| panic!("{source}: {e:#}"))
}

#[test]
fn an_empty_file_rebuilds_as_the_template() {
    assert_eq!(rebuilt("return {}"), template_for(&unix(Some("/d"), None)));
}

#[test]
fn a_rebuilt_file_loads_as_the_user_file_did() {
    let dir = abs("/c/custom").display().to_string();
    let custom_dir = format!("return {{ custom_dir = {dir:?} }}");
    for source in [
        "return { queue = 512 }",
        "return { version = '0.0.1', buffer_frames = 128 }",
        "return { tui = { meter_floor = -90, poll = 8.5 } }",
        "return { tui = { meter_latency = { render = { warn = 6 }, ceil = 50 } } }",
        "return { tui = { meter_fps = { danger = 12 } } }",
        "return { tui = { source = { channel = 2 }, spectrum = { size = 8192 } } }",
        "return { log = { trim_size_kb = 0 } }",
        "return { custom_dir = '' }",
        custom_dir.as_str(),
        "local floor = -48 return { tui = { meter_floor = floor, spectrum = { floor = floor * 2 } } }",
    ] {
        let rebuilt = rebuilt(source);
        let loaded = user(&rebuilt).unwrap();
        assert!(!loaded.outdated, "{rebuilt}");
        assert_eq!(
            loaded,
            Config {
                outdated: false,
                ..user(source).unwrap()
            },
            "{source}\n{rebuilt}"
        );
    }
}

#[test]
fn a_rebuilt_file_writes_values_in_place() {
    let rebuilt = rebuilt(
        "-- mine\nreturn { queue = 512, tui = { meter_latency = { render = { warn = 6 } } } }",
    );
    assert!(!rebuilt.contains("-- mine"), "{rebuilt}");
    assert!(
        rebuilt.contains("\tqueue = 512, -- buffer size"),
        "{rebuilt}"
    );
    assert!(
        rebuilt.contains("\t\t\trender = { warn = 6, danger = 10 },"),
        "{rebuilt}"
    );
    assert!(
        rebuilt.contains(&format!("\tversion = \"{}\",", built_in())),
        "{rebuilt}"
    );
}

#[test]
fn a_rebuilt_custom_dir_is_uncommented() {
    let rebuilt = rebuilt("return { custom_dir = '~/my \"custom\"' }");
    assert!(
        rebuilt.contains("\tcustom_dir = \"~/my \\\"custom\\\"\", -- custom folder"),
        "{rebuilt}"
    );
    assert!(!rebuilt.contains("-- custom_dir ="), "{rebuilt}");
}

#[test]
fn a_file_that_does_not_load_is_not_rebuilt() {
    assert!(rebuild_for("return { queue = 8 }", "test", &unix(Some("/d"), None)).is_err());
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
fn a_nested_budget_changes_only_what_it_sets() {
    let config = user("return { tui = { meter_latency = { render = { warn = 6 } } } }").unwrap();
    let defaults = resolve(None).unwrap();
    assert_eq!(config.tui.render_budget.warn, 6.0);
    assert_eq!(
        config.tui.render_budget.danger,
        defaults.tui.render_budget.danger
    );
    assert_eq!(config.tui.latency_window, defaults.tui.latency_window);
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

fn custom(setting: &str, xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    // an absolute setting is made absolute on the host, as `var` does
    let setting = match setting.starts_with('/') {
        true => abs(setting).display().to_string(),
        false => setting.to_string(),
    };
    custom_dir(&setting, &unix(xdg, home)).unwrap()
}

#[test]
fn the_custom_folder_defaults_to_the_data_dir() {
    assert_eq!(
        custom("", Some("/d"), Some("/h")),
        Some(abs("/d/synth/custom"))
    );
    assert_eq!(
        custom("", None, Some("/h")),
        Some(abs("/h/.local/share/synth/custom"))
    );
    assert_eq!(custom("", Some("d"), None), None);
}

#[test]
fn the_template_names_the_default_custom_folder() {
    // joined as the code joins it, so Windows gets its own separators
    let dir = abs("/d").join("synth").join("custom");
    let dir = dir.display().to_string();
    let written = template_for(&unix(Some("/d"), None));
    assert!(written.contains(&format!("-- custom_dir = [[{dir}]]")));
    assert!(!written.contains(DEFAULT_CUSTOM_DIR));

    let written = template_for(&unix(None, None));
    assert!(!written.contains("-- custom_dir ="));
}

#[test]
fn the_custom_folder_takes_absolute_and_home_paths() {
    assert_eq!(
        custom("/music/synth", None, Some("/h")),
        Some(abs("/music/synth"))
    );
    assert_eq!(custom("~/music", None, Some("/h")), Some(abs("/h/music")));
}

#[test]
fn windows_uses_appdata_for_config_and_custom() {
    let dirs = windows(Some("/appdata"), Some("/users/me"));
    assert_eq!(
        dirs.config().map(|dir| under(dir, &FILE)),
        Some(abs("/appdata/synth/config.lua"))
    );
    assert_eq!(
        custom_dir("", &dirs).unwrap(),
        Some(abs("/appdata/synth/custom"))
    );
    // with no HOME, ~/ is the user profile
    assert_eq!(
        custom_dir("~/music", &dirs).unwrap(),
        Some(abs("/users/me/music"))
    );
}

#[test]
fn xdg_wins_on_windows_too() {
    let dirs = Dirs {
        xdg_config: Some(var("/x/config")),
        xdg_data: Some(var("/x/data")),
        ..windows(Some("/appdata"), None)
    };
    assert_eq!(dirs.config(), Some(abs("/x/config")));
    assert_eq!(dirs.data(), Some(abs("/x/data")));
}

#[test]
fn userprofile_stands_in_for_home_only_on_windows() {
    let unix = Dirs {
        userprofile: Some(var("/users/me")),
        ..unix(None, None)
    };
    assert_eq!(unix.home(), None);
}

#[test]
fn a_relative_custom_folder_is_named() {
    let error = custom_dir("music", &unix(None, Some("/h"))).unwrap_err();
    assert!(
        matches!(&error, Error::CustomDirRelative { setting } if setting == "music"),
        "{error}"
    );
    let error = custom_dir("~/music", &unix(None, None)).unwrap_err();
    assert!(matches!(error, Error::CustomDirNoHome), "{error}");
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
        ("return { custom_dir = 5 }", "`custom_dir` must be a string"),
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
        (
            "return { tui = { meter_latency = { window = 0 } } }",
            "config.lua tui meter_latency: `window` is 0; must be above 0",
        ),
        (
            "return { tui = { meter_latency = { records = 0 } } }",
            "config.lua tui meter_latency: `records` is 0; must be 1 to 65536",
        ),
        (
            "return { log = { trim_size_kb = -1 } }",
            "config.lua log: `trim_size_kb` is -1; must be 0 to 1048576",
        ),
        (
            "return { tui = { faults = 0 } }",
            "config.lua tui: `faults` is 0; must be 1 to 65536",
        ),
        (
            "return { tui = { source = { stream = 'input' } } }",
            "config.lua tui source: `stream` is \"input\"; must be one of output",
        ),
        (
            "return { tui = { source = { channel = -1 } } }",
            "config.lua tui source: `channel` is -1; must be 0 to 64",
        ),
        (
            "return { tui = { source = { tee = 0 } } }",
            "config.lua tui source: `tee` is 0; must be 1 to 10000",
        ),
        (
            "return { tui = { scope = { cycles = 0 } } }",
            "config.lua tui scope: `cycles` is 0; must be 0.25 to 64",
        ),
        (
            "return { tui = { scope = { history = 0 } } }",
            "config.lua tui scope: `history` is 0; must be 1 to 10000",
        ),
        (
            "return { tui = { spectrum = { size = 128 } } }",
            "config.lua tui spectrum: `size` is 128; must be 256 to 32768",
        ),
        (
            "return { tui = { spectrum = { size = 3000 } } }",
            "config.lua tui spectrum: `size` is 3000; must be a power of two",
        ),
        (
            "return { tui = { spectrum = { floor = 0 } } }",
            "config.lua tui spectrum: `floor` is 0; must be below 0",
        ),
        (
            "return { tui = { spectrum = { height = 100 } } }",
            "config.lua tui spectrum: `height` is 100; must be 1 to 99",
        ),
        (
            "return { tui = { meter_latency = { records = 1.5 } } }",
            "`records` must be a whole number",
        ),
        (
            "return { tui = { meter_latency = { floor = 0 } } }",
            "`floor` is 0; must be above 0",
        ),
        (
            "return { tui = { meter_latency = { floor = 100 } } }",
            "config.lua tui meter_latency: `floor` is 100, `ceil` is 100; `floor` must be below `ceil`",
        ),
        (
            "return { tui = { meter_latency = { render = { warn = 10 } } } }",
            "config.lua tui meter_latency render: `warn` is 10, `danger` is 10; `warn` must be below `danger`",
        ),
        (
            "return { tui = { meter_latency = { input = { wran = 1 } } } }",
            "config.lua tui meter_latency input: unknown field `wran`",
        ),
        (
            "return { tui = { meter_fps = { danger = 30 } } }",
            "config.lua tui meter_fps: `danger` is 30, `warn` is 30; `danger` must be below `warn`",
        ),
        (
            "return { tui = { meter_latency = { windw = 1 } } }",
            "unknown field `windw`",
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

#[test]
fn a_config_that_does_not_run_is_matched_by_variant() {
    let error = user("return {").unwrap_err();
    assert!(
        matches!(&error, Error::Run { name, .. } if name == "test"),
        "{error}"
    );
    let error = user("return 256").unwrap_err();
    assert!(
        matches!(&error, Error::ReturnNotATable { what, got: "integer" } if what == "test"),
        "{error}"
    );
}
