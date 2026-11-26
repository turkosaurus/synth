//! The environment user Lua runs in: config, patches, and the modules they
//! load from disk. A strict allowlist, so anything not named here is gone.
//! The embedded modules run in the state's full globals instead.

use mlua::chunk::{AsChunk, Chunk, ChunkMode};
use mlua::{Lua, Table, Value};

/// Base functions kept as they are.
const BASE: [&str; 20] = [
    "pairs",
    "ipairs",
    "type",
    "tostring",
    "tonumber",
    "select",
    "error",
    "assert",
    "pcall",
    "xpcall",
    "setmetatable",
    "getmetatable",
    "rawget",
    "rawset",
    "rawequal",
    "rawlen",
    "next",
    "unpack",
    "print",
    // resolves only through the searcher `lua_register` installs
    "require",
];

/// Libraries kept as they are, shared with the embedded modules.
const LIBRARIES: [&str; 5] = ["string", "table", "math", "bit", "coroutine"];

/// Where the environment is kept, so every chunk in a state shares one.
const REGISTRY: &str = "synth.sandbox";

/// The sandbox environment for `lua`, made on first use.
pub(super) fn environment(lua: &Lua) -> mlua::Result<Table> {
    if let Some(env) = lua.named_registry_value::<Option<Table>>(REGISTRY)? {
        return Ok(env);
    }
    let globals = lua.globals();
    let env = lua.create_table()?;
    for name in BASE.iter().chain(&LIBRARIES) {
        // rawlen, bit and the like exist only on some Lua versions
        env.set(*name, globals.get::<Value>(*name)?)?;
    }
    // every string reaches this table through its metatable, so `dump`
    // goes from the library itself
    let string: Table = globals.get("string")?;
    string.set("dump", Value::Nil)?;
    let text = lua.create_function(load_text)?;
    env.set("load", text.clone())?;
    env.set("loadstring", text)?;
    env.set("_G", &env)?;
    lua.set_named_registry_value(REGISTRY, &env)?;
    Ok(env)
}

/// `source` compiled as a text chunk that runs in the sandbox.
pub(super) fn chunk<'a>(
    lua: &'a Lua,
    source: impl AsChunk + 'a,
    name: &str,
) -> mlua::Result<Chunk<'a>> {
    Ok(lua
        .load(source)
        .set_name(name)
        .set_mode(ChunkMode::Text)
        .set_environment(environment(lua)?))
}

/// `load` and `loadstring`: a string, or a function returning pieces of
/// one, compiled as text into the sandbox. A failure is returned as
/// `nil, message`, as Lua's own do.
fn load_text(
    lua: &Lua,
    (source, name): (Value, Option<String>),
) -> mlua::Result<(Value, Option<String>)> {
    let source = match source {
        Value::String(text) => text.as_bytes().to_vec(),
        Value::Function(reader) => {
            let mut bytes = Vec::new();
            loop {
                match reader.call::<Option<mlua::LuaString>>(())? {
                    Some(piece) if !piece.as_bytes().is_empty() => {
                        bytes.extend_from_slice(&piece.as_bytes());
                    }
                    _ => break,
                }
            }
            bytes
        }
        other => {
            return Err(mlua::Error::runtime(format!(
                "bad argument #1 to 'load' (string or function expected, got {})",
                other.type_name()
            )))
        }
    };
    let name = name.unwrap_or_else(|| "=(load)".to_string());
    match chunk(lua, source, &name)?.into_function() {
        Ok(function) => Ok((Value::Function(function), None)),
        Err(error) => Ok((Value::Nil, Some(error.to_string()))),
    }
}
