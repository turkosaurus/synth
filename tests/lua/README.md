# tests/lua

Lua tests fixtures evaluated by Rust tests, linted with `lint:lua` and `lint:stylua` 

| folder | what the test checks |
|---|---|
| `loader/` | how `require` resolves modules beside a patch and in a custom folder (`loader/custom/`); one folder per case |
| `bench/` | not a test: the patches `benches/render.rs` and `benches/instructions.rs` play, one per voice count, built by `chord.lua` |
| `lsp/` | each file gets the lua-language-server diagnostic on its first line, `-- expect: <code>`, in a custom folder (`lint:lsp`) |
| `reject/` | each file fails to load with the error on its first line, `-- expect: …`; `{file}` stands for the file's name. An optional second line, `-- expect-variant: Error::Variant { field: value, .. }`, matches the typed error it downcasts to: each listed field must equal that field's Debug text, and without `..` the listed fields must be all of them |
| `warn/` | each file loads with exactly the diagnostics its first lines list, one `-- expect: <kind> <line> <message>` each; `<line>` is `-` for none and `\t` is a tab |
| `cases/` | the Lua of Rust tests that assert computed values, loaded by name with `case("<name>")`; each file is named for its test |

A new `reject/` or `warn/` case is a new file; not Rust changes.
