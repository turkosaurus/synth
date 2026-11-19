# tests/lua

Lua that the Rust tests load, kept as files so `lint:lua` and `lint:stylua` check it like `lua/`.

| folder | what the test checks |
|---|---|
| `loader/` | how `require` resolves modules beside a patch; one folder per case |
| `reject/` | each file fails to load with the error on its first line, `-- expect: …`; `{file}` stands for the file's name |

A new `reject/` case is a new file; no Rust changes.
