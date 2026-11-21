# mise-tasks

Tasks long enough for their own file. Shorter ones live in [`mise.toml`](../mise.toml); `mise tasks` lists both.

| task | checks |
| --- | --- |
| [`test:separation`](./test/separation) | `mlua` only in `script`, `cpal` only in `audio/device.rs`, and `spec` names neither |
| [`test:core`](./test/core) | `src/audio/fx/` uses `core` alone: no `std`, no std-only float methods |
| [`lint:lua`](./lint/lua) | lua-language-server on `lua/` and `tests/lua/`, under LuaJIT and Lua 5.4 |
| [`lint:licenses`](./lint/licenses) | every dependency is GPLv3-compatible and from crates.io |
| [`version`](./version) | Cargo.toml's version is one bump from the last and has no tag yet |
| [`notices`](./notices) | writes third-party license notices for a release |
| [`bench:run`](./bench/run), alias `bench` | `bench:record`, then `bench:compare` on the result it saved |
| [`bench:record`](./bench/record) | times and counts instructions for the render path and writes `benches/results/<stamp>.toml` |
| [`bench:compare`](./bench/compare) | Markdown tables of the newest result: instructions against the last on the same arch, time against the last on the same CPU |

`test:separation` and `test:core` are text checks, not compiler guarantees: they hold the module boundaries the [architecture](../RULES.md#architecture) depends on.

Each task prints a `::error title=<task>::` line when it fails, so GitHub Actions annotates the run with the task's name.

| group | runs |
| --- | --- |
| `mise run tests` | every `test:*`, network included |
| `mise run test:local` | the tests that need no network |
| `mise run lint` | every linter; any warning fails, except `lint:pedantic` |
