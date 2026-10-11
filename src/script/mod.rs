use anyhow::Context;
use tracing::instrument;

mod bake;
mod feed;
mod keymap;

pub use bake::Report;

use feed::{sequence, single};
use keymap::keymap;

#[cfg(test)]
mod tests;

const LUA_SLOPE: &str = include_str!("../../lua/slope.lua");
const LUA_WAVE: &str = include_str!("../../lua/wave.lua");
const LUA_ENVELOPE: &str = include_str!("../../lua/envelope.lua");
const LUA_TUNING: &str = include_str!("../../lua/tuning.lua");
const LUA_VOICE: &str = include_str!("../../lua/voice.lua");
const LUA_SEQUENCE: &str = include_str!("../../lua/sequence.lua");
const LUA_KEYBOARD: &str = include_str!("../../lua/keyboard.lua");

// require checks package.preload before any filesystem search, so the
// embedded libraries resolve with no cwd dependency
fn lua_register(lua: &mlua::Lua) -> anyhow::Result<()> {
    let package: mlua::Table = lua.globals().get(field::PACKAGE)?;
    let preload: mlua::Table = package.get(field::PRELOAD)?;

    for (name, src) in [
        (module::SLOPE, LUA_SLOPE),
        (module::WAVE, LUA_WAVE),
        (module::ENVELOPE, LUA_ENVELOPE),
        (module::TUNING, LUA_TUNING),
        (module::VOICE, LUA_VOICE),
        (module::SEQUENCE, LUA_SEQUENCE),
        (module::KEYBOARD, LUA_KEYBOARD),
    ] {
        let loader = lua.create_function(move |lua, _: mlua::Value| {
            lua.load(src).set_name(name).eval::<mlua::Value>()
        })?;
        preload.set(name, loader)?;
    }
    Ok(())
}

// Every name crossing the Lua/Rust boundary, in one place.

/// Names the embedded modules are registered under. Used both to register
/// them and to require them, so the two cannot drift.
mod module {
    pub const SLOPE: &str = "synth.slope";
    pub const WAVE: &str = "synth.wave";
    pub const ENVELOPE: &str = "synth.envelope";
    pub const TUNING: &str = "synth.tuning";
    pub const VOICE: &str = "synth.voice";
    pub const SEQUENCE: &str = "synth.sequence";
    pub const KEYBOARD: &str = "synth.keyboard";
}

pub(super) mod field {
    pub const REQUIRE: &str = "require";
    pub const NEW: &str = "new";

    pub const WAVEFORM: &str = "waveform";
    pub const ENVELOPE: &str = "envelope";
    pub const KEYBOARD: &str = "keyboard";

    pub const ATTACK_MS: &str = "attackMS";
    pub const ATTACK_FUNC: &str = "attackFunc";
    pub const DECAY_MS: &str = "decayMS";
    pub const DECAY_FUNC: &str = "decayFunc";
    pub const SUSTAIN_LEVEL: &str = "sustainLevel";
    pub const RELEASE_MS: &str = "releaseMS";
    pub const RELEASE_FUNC: &str = "releaseFunc";

    pub const BPM: &str = "bpm";
    pub const DIVISION: &str = "division";
    pub const VOICES: &str = "voices";
    pub const STEPS: &str = "steps";
    pub const PATTERNS: &str = "patterns";
    pub const CHAIN: &str = "chain";
    pub const LOOP: &str = "loop";

    pub const FREQUENCY: &str = "frequency";

    // Lua's own, not ours: these can never be renamed, but a typo breaks
    // identically, and `PATH` is also written to
    pub const PACKAGE: &str = "package";
    pub const PRELOAD: &str = "preload";
    pub const PATH: &str = "path";
}

// Built from the consts above, so each name is spelled exactly once. Lua
// constructors pass unknown fields through rather than discarding them, so a
// misspelling arrives here and gets named instead of taking a default.

/// A loaded file: a playable timeline plus one report per voice in it.
pub struct Patch {
    pub timeline: crate::audio::Timeline,
    /// hz for every midi note, baked from Lua so temperament stays there
    pub freqs: Vec<f32>,
    pub keymap: crate::input::keyboard::Keymap,
    pub bpm: f32,
    pub division: f32,
    pub step_ms: f32,
    pub steps: usize,
    pub patterns: usize,
    pub chain: usize,
    pub loops: usize,
    pub reports: Vec<(String, Report)>,
}

impl Patch {
    /// No voice raised a warning, so the patch is safe to play as written.
    pub fn clean(&self) -> bool {
        self.reports.iter().all(|(_, report)| report.warnings.is_empty())
    }
}

/// What `voice.new({})` gives you, for running with no file.
const DEFAULT: &str = r#"
local voice = require("synth.voice")
return voice.new({})
"#;

#[instrument]
pub fn load_default(sample_rate: u32) -> anyhow::Result<Patch> {
    load_source(DEFAULT, "(default)", sample_rate)
}

#[instrument]
pub fn load(path: &std::path::Path, sample_rate: u32) -> anyhow::Result<Patch> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    load_in(
        &source,
        &path.display().to_string(),
        sample_rate,
        path.parent(),
    )
}

/// Same as [`load`] but from a string, so sequences can be tested without
/// fixture files on disk.
pub fn load_source(source: &str, name: &str, sample_rate: u32) -> anyhow::Result<Patch> {
    load_in(source, name, sample_rate, None)
}

fn load_in(
    source: &str,
    name: &str,
    sample_rate: u32,
    dir: Option<&std::path::Path>,
) -> anyhow::Result<Patch> {
    let lua = mlua::Lua::new();
    lua_register(&lua)?;

    // a patch's own requires resolve next to the patch, not next to the
    // shell's cwd. built through the package table rather than by splicing a
    // path into Lua source, which a path containing a quote would break.
    if let Some(dir) = dir {
        let package: mlua::Table = lua.globals().get(field::PACKAGE)?;
        let existing: String = package.get(field::PATH)?;
        let dir = dir.display();
        package.set(field::PATH, format!("{dir}/?.lua;{dir}/?/init.lua;{existing}"))?;
    }

    // eval to a Value first: a syntax error and a wrong return type both fail
    // this call, and they deserve different messages
    let returned: mlua::Value = lua.load(source).set_name(name).eval()?;
    let table = match returned {
        mlua::Value::Table(table) => table,
        other => anyhow::bail!("file must return a table, got {}", other.type_name()),
    };

    let require: mlua::Function = lua.globals().get(field::REQUIRE)?;
    let tuning: mlua::Table = require.call(module::TUNING)?;
    let to_hz: mlua::Function = tuning.get(field::FREQUENCY)?;

    let mut freqs = Vec::with_capacity(128);
    for note in 0..128 {
        let hz: f64 = to_hz.call(note)?;
        freqs.push(hz as f32);
    }

    // a patch may ship its own layout; otherwise the embedded default
    let layout: mlua::Table = match table.get(field::KEYBOARD)? {
        Some(table) => table,
        // the module exposes a constructor now, so the default layout is
        // keyboard.new() rather than the module table itself
        None => {
            let keyboard: mlua::Table = require.call(module::KEYBOARD)?;
            let new: mlua::Function = keyboard.get(field::NEW)?;
            new.call(())?
        }
    };
    let keys = keymap(&lua, &layout)?;

    let voice_specs: Option<mlua::Table> = table.get(field::VOICES)?;
    let steps: Option<mlua::Table> = table.get(field::STEPS)?;

    // bound rather than returned directly: the match tuple is a temporary
    // that would otherwise outlive `lua` and borrow it past its drop
    let patch = match (voice_specs, steps) {
        (Some(specs), Some(steps)) => {
            sequence(&lua, &table, &specs, &steps, sample_rate, freqs, keys)
        }
        // a bare voice file: one sustained note, so a two-event timeline
        _ => single(&table, sample_rate, freqs, keys),
    };
    patch
}
