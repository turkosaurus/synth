//! Lua in, `spec` out, and the only place `mlua` appears. Loads a patch,
//! checks it, and bakes it into [`crate::spec`] data; Lua is dropped once
//! the patch is returned.

use tracing::instrument;

mod bake;
pub mod builtins;
mod config;
mod error;
mod feed;
mod filter;
mod keymap;
mod sandbox;
mod schema;
pub mod starters;
mod structure;

use crate::spec::{Diagnostic, DiagnosticKind, Freqs, Patch};
#[cfg(test)]
pub(crate) use config::defaults as config_defaults;
pub use config::{config, config_path, log_path, socket_path, template as config_template};
pub use error::Error;
use feed::{bare, sequence, SINGLE};
use keymap::keymap;

#[cfg(test)]
mod tests;

const LUA_SLOPE: &str = include_str!("../lua/slope.lua");
const LUA_WAVE: &str = include_str!("../lua/wave.lua");
const LUA_ENVELOPE: &str = include_str!("../lua/envelope.lua");
const LUA_TUNING: &str = include_str!("../lua/tuning.lua");
const LUA_INSTRUMENT: &str = include_str!("../lua/instrument.lua");
const LUA_SEQUENCE: &str = include_str!("../lua/sequence.lua");
const LUA_KEYBOARD: &str = include_str!("../lua/keyboard.lua");
const LUA_FILTER: &str = include_str!("../lua/filter.lua");
const LUA_OSC: &str = include_str!("../lua/osc.lua");
const LUA_LFO: &str = include_str!("../lua/lfo.lua");
const LUA_ROOT: &str = include_str!("../lua/synth.lua");
const LUA_CONSTRUCT: &str = include_str!("../lua/construct.lua");

/// Every embedded module, by the name `require` finds it under.
const MODULES: [(&str, &str); 12] = [
    (module::CONSTRUCT, LUA_CONSTRUCT),
    (module::SLOPE, LUA_SLOPE),
    (module::WAVE, LUA_WAVE),
    (module::ENVELOPE, LUA_ENVELOPE),
    (module::TUNING, LUA_TUNING),
    (module::INSTRUMENT, LUA_INSTRUMENT),
    (module::SEQUENCE, LUA_SEQUENCE),
    (module::KEYBOARD, LUA_KEYBOARD),
    (module::FILTER, LUA_FILTER),
    (module::OSC, LUA_OSC),
    (module::LFO, LUA_LFO),
    (module::ROOT, LUA_ROOT),
];

/// Installs the one module searcher `require` uses, in place of Lua's own,
/// so nothing resolves from the shell's cwd, the system's Lua folders, or a
/// native library.
///
/// Every name, `synth.*` included, is looked for in `dir`, then in
/// `custom`, then among the embedded modules. On disk `a.b` is `a/b.lua`
/// or `a/b/init.lua`.
fn lua_register(
    lua: &mlua::Lua,
    dir: Option<std::path::PathBuf>,
    custom: Option<std::path::PathBuf>,
) -> Result<(), Error> {
    let package: mlua::Table = lua.globals().get(field::PACKAGE)?;

    let searcher = lua.create_function(move |lua, name: String| -> mlua::Result<mlua::Value> {
        let Some(relative) = module_path(&name) else {
            return Ok(mlua::Value::String(lua.create_string(format!(
                "\n\t'{name}' is not a module name: use letters, digits, `_` and `-`, \
                 separated by dots"
            ))?));
        };
        let embedded = MODULES.iter().find(|(module, _)| *module == name);
        let mut tried = String::new();
        let mut found = match &dir {
            Some(dir) => find(dir, &relative, &mut tried)?,
            None => None,
        };
        if let (None, Some(custom)) = (&found, &custom) {
            found = if embedded.is_some() {
                // a copy still matching its built-in is skipped, so an
                // updated built-in is not hidden by a stale copy
                builtins::edited(custom, &name)
            } else {
                find(custom, &relative, &mut tried)?
            };
        }
        if let Some((path, source)) = found {
            let chunk = sandbox::chunk(lua, source, &format!("@{}", path.display()))?;
            return Ok(mlua::Value::Function(chunk.into_function()?));
        }
        Ok(match embedded {
            Some((module, src)) => {
                mlua::Value::Function(lua.load(*src).set_name(*module).into_function()?)
            }
            None => mlua::Value::String(lua.create_string(tried)?),
        })
    })?;
    // LuaJIT is Lua 5.1, where the searchers are `package.loaders`
    package.set(field::LOADERS, lua.create_sequence_from([searcher])?)?;
    // unused once the searchers are replaced; cleared so nothing reads them
    package.set(field::PATH, "")?;
    package.set(field::CPATH, "")?;

    // `synth.site` is a Rust function, not Lua: it names the line that
    // called a constructor by reading the call stack directly, so no patch
    // ever needs Lua's debug library, which mlua leaves out anyway
    let loaded: mlua::Table = package.get(field::LOADED)?;
    loaded.set(module::SITE, lua.create_function(|lua, ()| Ok(site(lua)))?)?;
    Ok(())
}

/// Where a value was made when no Lua line called its constructor.
const BUILT_IN: &str = "built in";

/// `file:line` of the code that called a constructor. Level 0 is this
/// function, 1 the constructor, 2 whoever called the constructor.
fn site(lua: &mlua::Lua) -> String {
    // called from Rust, as the default keyboard is
    at(lua, 2).unwrap_or_else(|| BUILT_IN.to_string())
}

/// `file:line` of the function `level` calls up the stack, or `None` when
/// the stack is not that deep.
fn at(lua: &mlua::Lua, level: usize) -> Option<String> {
    lua.inspect_stack(level, |debug| {
        let source = debug.source();
        // `short_src` cuts a long path short; `source` keeps all of it
        let file = match source.source.as_deref().and_then(|s| s.strip_prefix('@')) {
            Some(path) => path.to_string(),
            None => {
                let short = source.short_src.unwrap_or_default();
                // embedded modules show as [string "synth.instrument"]
                short
                    .strip_prefix("[string \"")
                    .and_then(|rest| rest.strip_suffix("\"]"))
                    .unwrap_or(&short)
                    .to_string()
            }
        };
        match debug.current_line() {
            Some(line) => format!("{file}:{line}"),
            None => file,
        }
    })
}

// Every name crossing the Lua/Rust boundary, in one place.

/// Names the embedded modules are registered under. Used both to register
/// them and to require them, so the two cannot drift.
mod module {
    /// The helper every constructor builds its value with; not re-exported.
    pub(super) const CONSTRUCT: &str = "synth.construct";
    pub(super) const SLOPE: &str = "synth.slope";
    pub(super) const WAVE: &str = "synth.wave";
    pub(super) const ENVELOPE: &str = "synth.envelope";
    pub(super) const TUNING: &str = "synth.tuning";
    pub(super) const INSTRUMENT: &str = "synth.instrument";
    pub(super) const SEQUENCE: &str = "synth.sequence";
    pub(super) const KEYBOARD: &str = "synth.keyboard";
    pub(super) const FILTER: &str = "synth.filter";
    pub(super) const OSC: &str = "synth.osc";
    pub(super) const LFO: &str = "synth.lfo";
    /// Re-exports every other module, so a patch needs one `require`.
    pub(super) const ROOT: &str = "synth";
    /// a Rust function, set straight into `package.loaded`
    pub(super) const SITE: &str = "synth.site";
}

mod field {
    pub(super) const REQUIRE: &str = "require";

    pub(super) const OSCILLATOR: &str = "oscillator";
    pub(super) const WAVEFORMS: &str = "waveforms";
    pub(super) const SLOPES: &str = "slopes";
    pub(super) const POSITION: &str = "position";
    pub(super) const POSITION_MOD: &str = "position_mod";
    pub(super) const WAVE: &str = "wave";
    pub(super) const RATE: &str = "rate";
    pub(super) const DEPTH: &str = "depth";
    pub(super) const DEPTH_HZ: &str = "depth_hz";
    pub(super) const DEPTH_OCTAVES: &str = "depth_octaves";
    pub(super) const ENVELOPE: &str = "envelope";
    pub(super) const KEYBOARD: &str = "keyboard";
    pub(super) const VOICE_COUNT: &str = "voice_count";

    pub(super) const ATTACK: &str = "attack";
    pub(super) const ATTACK_CURVE: &str = "attack_curve";
    pub(super) const DECAY: &str = "decay";
    pub(super) const DECAY_CURVE: &str = "decay_curve";
    pub(super) const SUSTAIN: &str = "sustain";
    pub(super) const RELEASE: &str = "release";
    pub(super) const RELEASE_CURVE: &str = "release_curve";

    pub(super) const BPM: &str = "bpm";
    pub(super) const DIVISION: &str = "division";
    pub(super) const INSTRUMENTS: &str = "instruments";
    pub(super) const STEPS: &str = "steps";
    pub(super) const PATTERNS: &str = "patterns";
    pub(super) const CHAIN: &str = "chain";
    pub(super) const LOOP: &str = "loop";

    pub(super) const NOTE: &str = "note";
    pub(super) const OFF: &str = "off";
    pub(super) const INSTRUMENT: &str = "instrument";
    pub(super) const DURATION: &str = "duration";
    pub(super) const VELOCITY: &str = "velocity";

    pub(super) const LAYOUT: &str = "layout";
    pub(super) const STOP: &str = "stop";
    pub(super) const GATE: &str = "gate";
    pub(super) const AUTOREPEAT_THRESHOLD: &str = "autorepeat_threshold";
    pub(super) const DOUBLETAP_THRESHOLD: &str = "doubletap_threshold";
    pub(super) const OCTAVE: &str = "octave";
    pub(super) const OCTAVE_LOW: &str = "octave_low";
    pub(super) const OCTAVE_HIGH: &str = "octave_high";

    pub(super) const TUNING: &str = "tuning";
    pub(super) const EQUAL: &str = "equal";

    pub(super) const FILTER: &str = "filter";
    // reserved: stamped by the synth.filter constructors, never by users
    pub(super) const KIND: &str = "_kind";

    // what constructors stamp on the metatable of the table they return
    pub(super) const STAMP_KIND: &str = "kind";
    pub(super) const STAMP_AT: &str = "at";
    pub(super) const CUTOFF: &str = "cutoff";
    pub(super) const CUTOFF_MOD: &str = "cutoff_mod";
    pub(super) const RESONANCE: &str = "resonance";
    pub(super) const GAIN: &str = "gain";

    // Lua's own, not ours: these can never be renamed, but a typo breaks
    // identically, and `PATH` is also written to
    pub(super) const CALL: &str = "__call";
    pub(super) const PACKAGE: &str = "package";

    pub(super) const BUFFER_FRAMES: &str = "buffer_frames";
    pub(super) const QUEUE: &str = "queue";
    pub(super) const CUSTOM_DIR: &str = "custom_dir";
    pub(super) const TUI: &str = "tui";
    pub(super) const LOG: &str = "log";
    pub(super) const TRIM_SIZE_KB: &str = "trim_size_kb";
    pub(super) const POLL: &str = "poll";
    pub(super) const METER_HOLD: &str = "meter_hold";
    pub(super) const METER_FALL: &str = "meter_fall";
    pub(super) const METER_FLOOR: &str = "meter_floor";
    pub(super) const METER_LATENCY: &str = "meter_latency";
    pub(super) const METER_FPS: &str = "meter_fps";
    pub(super) const FAULTS: &str = "faults";
    pub(super) const WINDOW: &str = "window";
    pub(super) const RECORDS: &str = "records";
    pub(super) const FLOOR: &str = "floor";
    pub(super) const CEIL: &str = "ceil";
    pub(super) const INPUT: &str = "input";
    pub(super) const RENDER: &str = "render";
    pub(super) const OUTPUT: &str = "output";
    pub(super) const TOTAL: &str = "total";
    pub(super) const WARN: &str = "warn";
    pub(super) const DANGER: &str = "danger";
    pub(super) const LOADERS: &str = "loaders";
    pub(super) const CPATH: &str = "cpath";
    pub(super) const LOADED: &str = "loaded";
    pub(super) const PATH: &str = "path";
}

/// One folder per kind in the custom folder, so
/// `instruments/bass.lua` is `require("instruments.bass")`.
pub const PATCH_KINDS: [&str; 5] = ["instruments", "effects", "keyboards", "tunings", "patterns"];

/// What plays when no file is given.
const DEFAULT: &str = include_str!("../lua/default.lua");

/// The default patch. `custom` is the user's custom folder, searched by
/// `require`, as for [`load`].
#[instrument]
pub(crate) fn load_default(
    sample_rate: u32,
    custom: Option<&std::path::Path>,
) -> anyhow::Result<Patch> {
    load_in(DEFAULT, "(default)", sample_rate, None, custom)
}

/// The patch at `path`. `require` looks beside it first, then in `custom`,
/// the user's custom folder from `config.lua`.
#[instrument]
pub fn load(
    path: &std::path::Path,
    sample_rate: u32,
    custom: Option<&std::path::Path>,
) -> anyhow::Result<Patch> {
    let source =
        std::fs::read_to_string(path).map_err(|source| Error::io("reading", path, source))?;
    load_in(
        &source,
        &path.display().to_string(),
        sample_rate,
        path.parent(),
        custom,
    )
}

/// Same as [`load`] but from a string, so sequences can be tested without
/// fixture files on disk.
#[cfg(test)]
pub(crate) fn load_source(source: &str, name: &str, sample_rate: u32) -> anyhow::Result<Patch> {
    load_in(source, name, sample_rate, None, None)
}

/// The source of `relative` under `root`, as a `.lua` file or an
/// `init.lua` in its folder. Paths not found are added to `tried`.
fn find(
    root: &std::path::Path,
    relative: &std::path::Path,
    tried: &mut String,
) -> mlua::Result<Option<(std::path::PathBuf, String)>> {
    let file = root.join(relative).with_extension("lua");
    let init = root.join(relative).join("init.lua");
    for path in [file, init] {
        match std::fs::read_to_string(&path) {
            Ok(source) => return Ok(Some((path, source))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tried.push_str(&format!("\n\tno file '{}'", path.display()));
            }
            Err(error) => {
                return Err(mlua::Error::external(format!(
                    "reading {}: {error}",
                    path.display()
                )))
            }
        }
    }
    Ok(None)
}

/// `a.b` as the relative path `a/b`, or `None` when a part is empty or
/// holds anything but letters, digits, `_` and `-`, so a name cannot reach
/// outside the folder it is looked for in.
fn module_path(name: &str) -> Option<std::path::PathBuf> {
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    };
    name.split('.')
        .all(valid)
        .then(|| name.split('.').collect())
}

fn load_in(
    source: &str,
    name: &str,
    sample_rate: u32,
    dir: Option<&std::path::Path>,
    custom: Option<&std::path::Path>,
) -> anyhow::Result<Patch> {
    let lua = mlua::Lua::new();
    // a patch in the custom folder itself finds built-in copies only
    // through the edited check, so the folder is not searched twice
    let dir = dir.filter(|dir| Some(*dir) != custom);
    lua_register(
        &lua,
        dir.map(std::path::Path::to_path_buf),
        custom.map(std::path::Path::to_path_buf),
    )?;

    // eval to a Value first: a syntax error and a wrong return type both fail
    // this call, and they deserve different messages
    // `@` makes Lua show the name as a file: `e.lua:2:`, not
    // `[string "e.lua"]:2:`
    let returned: mlua::Value = sandbox::chunk(&lua, source, &format!("@{name}"))?
        .eval()
        .map_err(Error::from)?;
    let table = match returned {
        mlua::Value::Table(table) => table,
        other => {
            return Err(Error::ReturnNotATable {
                what: "file".to_string(),
                got: other.type_name(),
            }
            .into())
        }
    };

    // checked before anything is read, so a wrong value is named by its place
    // in the patch and the line that built it, not by the field that tripped
    let structure = structure::walk(&table, name)?;

    let require: mlua::Function = lua.globals().get(field::REQUIRE)?;

    // a patch may ship its own tuning; otherwise equal temperament at A440
    let to_hz: mlua::Function = match table.get(field::TUNING)? {
        Some(function) => function,
        None => {
            let tuning: mlua::Table = require.call(module::TUNING)?;
            let equal: mlua::Function = tuning.get(field::EQUAL)?;
            equal.call(())?
        }
    };

    let (freqs, tuning_diagnostics) = tune(&to_hz, sample_rate)?;

    // a patch may ship its own layout; otherwise the embedded default
    let layout: mlua::Table = match table.get(field::KEYBOARD)? {
        Some(table) => table,
        // the module is itself the constructor, so the default layout is
        // keyboard() rather than the module table
        None => {
            let keyboard: mlua::Table = require.call(module::KEYBOARD)?;
            // a callable table: __call receives the module itself first
            let call: mlua::Function = keyboard
                .metatable()
                .ok_or(Error::NotCallable {
                    what: "synth.keyboard",
                })?
                .get(field::CALL)?;
            call.call((keyboard, mlua::Value::Nil))?
        }
    };
    let keys = keymap(&lua, &layout)?;

    // read before the instrument, which only checks that the field is allowed
    let filter: Option<filter::FilterSpec> = table.get(field::FILTER)?;

    let instrument_specs: Option<mlua::Table> = table.get(field::INSTRUMENTS)?;
    let steps: Option<mlua::Table> = table.get(field::STEPS)?;

    // bound rather than returned directly: the match tuple is a temporary
    // that would otherwise outlive `lua` and borrow it past its drop
    let patch = match (instrument_specs, steps) {
        (Some(specs), Some(steps)) => {
            sequence(&lua, &table, &specs, &steps, sample_rate, freqs, keys)
        }
        _ => bare(
            lua.unpack(mlua::Value::Table(table)).map_err(Error::from)?,
            sample_rate,
            freqs,
            keys,
        ),
    };
    let (filter, filter_report) = filter
        .map(|spec| spec.bake(sample_rate))
        .transpose()?
        .unzip();
    let mut patch = Patch {
        structure,
        tuning_diagnostics,
        printed: sandbox::printed(&lua),
        filter_report,
        filter,
        ..patch?
    };
    locate(&mut patch);
    Ok(patch)
}

/// Hz for every MIDI note from the patch's tuning.
///
/// A frequency that is not positive and finite stops the load: it would
/// reach the oscillator as a NaN or a frozen phase. One above Nyquist is
/// only a warning, since it plays, folded back to a lower pitch.
fn tune(to_hz: &mlua::Function, sample_rate: u32) -> Result<(Freqs, Vec<Diagnostic>), Error> {
    let notes = (0..128u8).map(f64::from);
    // the check passes only values the f32 table holds, so none clamp
    let (sampled, _) = bake::sample(notes, 0.0..=f64::from(f32::MAX), |x| {
        // an integer, not 69.0, so `math.type` and `tostring` see a note
        let note = x as u8;
        let hz: f64 = to_hz.call(note).map_err(|source| Error::TuningCall {
            note,
            source: error::trimmed(source),
        })?;
        // checked after narrowing, so a value too large for f32 is caught
        let narrowed = hz as f32;
        if !(narrowed.is_finite() && narrowed > 0.0) {
            return Err(Error::TuningHz { note, hz });
        }
        Ok(hz)
    })?;
    let mut freqs = [0.0; 128];
    for (slot, hz) in freqs.iter_mut().zip(sampled) {
        *slot = hz;
    }

    let nyquist = sample_rate as f32 / 2.0;
    let mut diagnostics = Vec::new();
    // tunings rise with the note, but nothing requires it, so every note is
    // checked rather than the first one over
    let over: Vec<u8> = (0..128u8)
        .filter(|&note| freqs[note as usize] > nyquist)
        .collect();
    if let (Some(first), Some(last)) = (over.first(), over.last()) {
        let notes = if first == last {
            format!("note {first} is")
        } else {
            format!("{} notes from {first} to {last} are", over.len())
        };
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::Range,
            format!("tuning: {notes} above Nyquist ({nyquist} Hz at {sample_rate} Hz) and fold to lower pitches"),
        ));
    }
    Ok((Freqs::new(freqs)?, diagnostics))
}

/// Gives every diagnostic the `file:line` of the constructor that built the
/// value it is about, from the structure walk.
fn locate(patch: &mut Patch) {
    let structure = &patch.structure;
    let at = |path: &str| {
        structure
            .iter()
            .find(|node| node.path == path)
            .map(|node| node.at.clone())
    };
    for (name, report) in &mut patch.reports {
        let path = if name == SINGLE {
            String::new()
        } else {
            format!("instruments.{name}")
        };
        let place = at(&path);
        for diagnostic in &mut report.diagnostics {
            diagnostic.at = diagnostic.at.take().or_else(|| place.clone());
        }
    }
    if let Some(report) = &mut patch.filter_report {
        let place = at("filter");
        for diagnostic in &mut report.diagnostics {
            diagnostic.at = diagnostic.at.take().or_else(|| place.clone());
        }
    }
}
