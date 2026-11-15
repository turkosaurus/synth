use anyhow::Context;
use tracing::instrument;

mod bake;
mod feed;
mod filter;
mod keymap;
mod structure;

use crate::spec::Patch;
use feed::{sequence, single};
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
const LUA_ROOT: &str = include_str!("../lua/synth.lua");

/// Every embedded module, by the name `require` finds it under.
const MODULES: [(&str, &str); 9] = [
    (module::SLOPE, LUA_SLOPE),
    (module::WAVE, LUA_WAVE),
    (module::ENVELOPE, LUA_ENVELOPE),
    (module::TUNING, LUA_TUNING),
    (module::INSTRUMENT, LUA_INSTRUMENT),
    (module::SEQUENCE, LUA_SEQUENCE),
    (module::KEYBOARD, LUA_KEYBOARD),
    (module::FILTER, LUA_FILTER),
    (module::ROOT, LUA_ROOT),
];

// require checks package.preload before any filesystem search, so the
// embedded libraries resolve with no cwd dependency
fn lua_register(lua: &mlua::Lua) -> anyhow::Result<()> {
    let package: mlua::Table = lua.globals().get(field::PACKAGE)?;
    let preload: mlua::Table = package.get(field::PRELOAD)?;

    for (name, src) in MODULES {
        let loader = lua.create_function(move |lua, _: mlua::Value| {
            lua.load(src).set_name(name).eval::<mlua::Value>()
        })?;
        preload.set(name, loader)?;
    }

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
    lua.inspect_stack(2, |debug| {
        let source = debug.source().short_src.unwrap_or_default();
        // chunks named by mlua show as [string "lua/example.lua"]
        let file = source
            .strip_prefix("[string \"")
            .and_then(|rest| rest.strip_suffix("\"]"))
            .unwrap_or(&source)
            .to_string();
        match debug.current_line() {
            Some(line) => format!("{file}:{line}"),
            None => file,
        }
    })
    // called from Rust, as the default keyboard is
    .unwrap_or_else(|| BUILT_IN.to_string())
}

// Every name crossing the Lua/Rust boundary, in one place.

/// Names the embedded modules are registered under. Used both to register
/// them and to require them, so the two cannot drift.
mod module {
    pub(super) const SLOPE: &str = "synth.slope";
    pub(super) const WAVE: &str = "synth.wave";
    pub(super) const ENVELOPE: &str = "synth.envelope";
    pub(super) const TUNING: &str = "synth.tuning";
    pub(super) const INSTRUMENT: &str = "synth.instrument";
    pub(super) const SEQUENCE: &str = "synth.sequence";
    pub(super) const KEYBOARD: &str = "synth.keyboard";
    pub(super) const FILTER: &str = "synth.filter";
    /// Re-exports every other module, so a patch needs one `require`.
    pub(super) const ROOT: &str = "synth";
    /// a Rust function, set straight into `package.loaded`
    pub(super) const SITE: &str = "synth.site";
}

mod field {
    pub(super) const REQUIRE: &str = "require";

    pub(super) const WAVEFORM: &str = "waveform";
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

    pub(super) const TUNING: &str = "tuning";
    pub(super) const EQUAL: &str = "equal";

    pub(super) const FILTER: &str = "filter";
    // reserved: stamped by the synth.filter constructors, never by users
    pub(super) const KIND: &str = "_kind";

    // what constructors stamp on the metatable of the table they return
    pub(super) const STAMP_KIND: &str = "kind";
    pub(super) const STAMP_AT: &str = "at";
    pub(super) const CUTOFF: &str = "cutoff";
    pub(super) const RESONANCE: &str = "resonance";
    pub(super) const GAIN: &str = "gain";

    // Lua's own, not ours: these can never be renamed, but a typo breaks
    // identically, and `PATH` is also written to
    pub(super) const CALL: &str = "__call";
    pub(super) const PACKAGE: &str = "package";
    pub(super) const PRELOAD: &str = "preload";
    pub(super) const LOADED: &str = "loaded";
    pub(super) const PATH: &str = "path";
}

/// What plays when no file is given.
const DEFAULT: &str = include_str!("../lua/default.lua");

#[instrument]
pub(crate) fn load_default(sample_rate: u32) -> anyhow::Result<Patch> {
    load_source(DEFAULT, "(default)", sample_rate)
}

#[instrument]
pub fn load(path: &std::path::Path, sample_rate: u32) -> anyhow::Result<Patch> {
    let source =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    load_in(
        &source,
        &path.display().to_string(),
        sample_rate,
        path.parent(),
    )
}

/// Same as [`load`] but from a string, so sequences can be tested without
/// fixture files on disk.
pub(crate) fn load_source(source: &str, name: &str, sample_rate: u32) -> anyhow::Result<Patch> {
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
        package.set(
            field::PATH,
            format!("{dir}/?.lua;{dir}/?/init.lua;{existing}"),
        )?;
    }

    // eval to a Value first: a syntax error and a wrong return type both fail
    // this call, and they deserve different messages
    let returned: mlua::Value = lua.load(source).set_name(name).eval()?;
    let table = match returned {
        mlua::Value::Table(table) => table,
        other => anyhow::bail!("file must return a table, got {}", other.type_name()),
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

    let mut freqs = Vec::with_capacity(128);
    for note in 0..128 {
        let hz: f64 = to_hz
            .call(note)
            .with_context(|| format!("tuning must return Hz for note {note}"))?;
        freqs.push(hz as f32);
    }

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
                .context("synth.keyboard is not callable")?
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
        // a bare instrument file: one sustained note, so a two-event timeline
        _ => single(
            lua.unpack(mlua::Value::Table(table))?,
            sample_rate,
            freqs,
            keys,
        ),
    };
    Ok(Patch {
        structure,
        filter_report: filter.as_ref().map(|spec| spec.report(sample_rate)),
        filter: filter.map(|spec| spec.filter),
        ..patch?
    })
}
