# tests/lua

Lua tests fixtures evaluated by Rust tests, linted with `lint:lua` and `lint:stylua` 

| folder | what the test checks |
|---|---|
| `loader/` | how `require` resolves modules beside a patch and in a patches folder (`loader/patches/`); one folder per case |
| `bench/` | not a test: the patches `benches/render.rs` and `benches/instructions.rs` play, one per voice count, built by `chord.lua` |
| `reject/` | each file fails to load with the error on its first line, `-- expect: …`; `{file}` stands for the file's name |

A new `reject/` case is a new file; not Rust changes.
