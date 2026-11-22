# synth
[![test](https://github.com/turkosaurus/synth/actions/workflows/test.yml/badge.svg)](https://github.com/turkosaurus/synth/actions/workflows/test.yml)
![GitHub Release](https://img.shields.io/github/v/release/turkosaurus/synth)

_Lua-defined, Rust-driven synthesizer and sequencer_

## about

A real-time Rust digital instrument and audio workstation configured and driven by Lua functions and tables.
- Design principles: [`RULES.md`](RULES.md)
- Development process: [`CONTRIBUTING.md`](CONTRIBUTING.md)

## install

### Linux[^requires]/MacOS

[`install.sh`](./install.sh) downloads and verifies the latest stable release, installing it to `$XDG_BIN_HOME`, or `~/.local/bin`[^manager].

```sh
curl -fsSL https://raw.githubusercontent.com/turkosaurus/synth/main/install.sh | bash
```

[^manager]: `update` and `uninstall` act on the binary being run; defer to package manager if installed through one.

[^requires]: requires `GLIBC_2.34+`


## run

### tui
```sh
synth tui
```

### components

```mermaid
flowchart LR
    lua[(lua)] --> script
    subgraph rust
        script -->|validate| spec
        spec -->|keymap| input
        spec --> |config| session
        input -->|event| session
        view -->|msg| session
        view -->|read| session
        session -->|play| events
        spec -->|sequence| events
        subgraph audio
            events --> render
        end
    end
    subgraph hardware
        controller["controller (keyboard, MIDI)"]
        speakers
        display
    end
    controller --> input
    view -->|draw| display
    render --> speakers
```

details: [`lua`](./lua), [`script`](./src/script), [`spec`](./src/spec), [`input`](./src/input), [`session`](./src/session.rs), [`audio`](./src/audio), [`view`](./src/view), or [`src`](./src)

## contributing
- see [`CONTRIBUTING.md`](CONTRIBUTING.md)

### roadmap
- [v1.0 issues](https://github.com/turkosaurus/synth/milestone/1)
- [kanban](https://github.com/users/turkosaurus/projects/4/views/2)
