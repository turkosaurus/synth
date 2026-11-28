# mise-tasks

Tasks long enough for their own file. Shorter ones live in [`mise.toml`](../mise.toml); `mise tasks` lists both.

| task | checks |
| --- | --- |
| [`test:separation`](./test/separation) | `mlua` only in `script`, `cpal` only in `audio/device.rs`, and `spec` names neither |
| [`test:core`](./test/core) | `src/audio/fx/` uses `core` alone: no `std`, no std-only float methods |
| [`lint:lua`](./lint/lua) | lua-language-server on `lua/` and `tests/lua/`, under LuaJIT and Lua 5.4 |
| [`lint:lsp`](./lint/lsp) | in a custom folder from this build, lua-language-server flags each `tests/lua/lsp/` case and nothing else, and a headless Neovim ([`tests/nvim/lsp.lua`](../tests/nvim/lsp.lua)) gets the root, hover, and completion the annotations give |
| [`lsp:edit`](./lsp/edit) | opens that custom folder, at `target/lsp/`, in `$VISUAL` or `$EDITOR`, to try the annotations by hand |
| [`lint:config`](./lint/config) | `lua/config.lua`, when it differs from `VERSION_BASE` (default `origin/main`), names Cargo.toml's version |
| [`lint:licenses`](./lint/licenses) | every dependency is GPLv3-compatible and from crates.io |
| [`lint:ps1`](./lint/ps1) | PSScriptAnalyzer on `install.ps1` and `tests/winstall.ps1`; skipped without `pwsh`, except in CI |
| [`test:winstall`](./test/winstall) | `install.ps1` from the latest release under Windows PowerShell and PowerShell 7: install, `--version`, `update --dev`, `update` back to stable, uninstall, nothing left; Windows only, run by `release.yml` |
| [`lint:shell`](./lint/shell) | shellcheck on every shell file task |
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
