# AGENTS.md

How work on synth is done, for anyone (person or agent) changing this repo. The architecture itself is in the [README](./README.md#architecture).

## principles

- Lua is the control surface. Every default and musical choice a player would read or change lives in Lua, in each module's `defaults` table.
- Rust owns what is valid (ranges such as MIDI 0 to 127), what must work when Lua fails (quitting, fallbacks), and how data is laid out for the audio thread. Valid ranges are declared once in [`schema.rs`](./src/script/schema.rs), and a test checks every Lua default against them.
- No musical value is hard-coded in Rust, and no Lua source is held in a Rust string; embed `.lua` files with `include_str!`. Lua inside test helpers is the one exception.
- The audio callback never allocates, locks, or blocks. `mise run test` enforces where `mlua` and `cpal` may appear.

## Lua conventions

- snake_case names; audio time in milliseconds with no unit in the name (`attack = 20`)
- one type per field; detail goes in another field, not an alternative shape (`release` and `release_curve`)
- constructors are called directly and return a new table, leaving the caller's alone: `synth.envelope({ attack = 20 })`
- a patch returns `synth.instrument({...})` or `synth.sequence({...})`; [`default.lua`](./lua/default.lua) is what plays with no file, written out in full as a template

## workflow

- Work from a GitHub issue: read it, its parent epic and every comment, and the issues it links, then follow its PR checklist.
- One small PR per checklist item, each from its own branch and worktree, opened as a draft. The maintainer reviews and merges.
- Prefer behaviour-neutral refactors first, then add the new API, move callers to it, and remove the old one, with no compatibility shims left at the end.
- Stacked PRs target the branch below them. When the lower one merges, retarget the next to `main` before merging it, or it lands on the merged branch and never reaches `main`.
- Catch a branch up by merging `main` into it. Never rebase pushed history or force push.
- `closes` in a PR only fires when it merges into `main`; tick and close the issue by hand otherwise.

## checks

Run before every commit:

- `mise run test`: Rust tests, the module separation check, and the example patch
- `mise run lint`: clippy with warnings denied, lua-language-server, and stylua

## releases

- Bump the version only in a PR, with `mise run bump patch` (or `minor`, `major`). CI checks the bump in the PR.
- Merging a bump to `main` builds every target, tags `vX.Y.Z`, and publishes the GitHub release.

## issues and PRs

- Write them in the [write-tech](https://github.com/turkosaurus/dotfiles/blob/main/home/.claude/skills/write-tech/SKILL.md) style: a one-sentence summary first, short bullets, and every claim about the code linked to the lines it describes.
- Put each issue or PR reference on its own list item as a full URL, so GitHub shows its title; never an inline `#N`.
- Every issue has a type (Feature, Bug, or Task) and an area label (`audio`, `os`, `ui`). Epics are type Task with the `epic` label, and every other issue sits under one.
