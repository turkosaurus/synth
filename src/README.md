# src

| module | purpose |
| --- | --- |
| [`main.rs`](./main.rs) | CLI: parses a command and dispatches it |
| [`script`](./script) | Lua in, `spec` out |
| [`spec`](./spec) | plain data every module shares; names nothing else in the crate |
| [`audio`](./audio) | the realtime path: render and device |
| [`input`](./input) | character keyboard to note events |
| [`session.rs`](./session.rs) | model, msg, and cmd for a live view; wires device, patch, and ring |
| [`view`](./view) | tui, reports, prompts |
| [`terminal.rs`](./terminal.rs) | raw mode and key release detection |
| [`log.rs`](./log.rs) | the live log at `$XDG_RUNTIME_DIR/synth/log.sock`, and following it or `log.jsonl` for `log` |
| [`install.rs`](./install.rs) | `install`, `uninstall` |
| [`update.rs`](./update.rs) | `update` from GitHub releases |
| [`release.rs`](./release.rs) | build identity for `--version` |

```mermaid
flowchart LR
    subgraph patch [patch commands]
        doctor([doctor])
        check([check])
        config([config])
        custom([custom])
        play([play])
        tui([tui])
        log_cmd([log])
    end
    subgraph binary [binary commands]
        install_cmd([install])
        uninstall([uninstall])
        update_cmd([update])
        version([version])
    end
    patch --> script & view
    doctor & tui --> terminal
    play --> audio
    tui --> session --> script & audio & input
    log_cmd --> log & view
    install_cmd & uninstall --> install
    update_cmd --> update --> install & release
    version --> release
```

`mlua` stays in `script`, `cpal` in `audio/device.rs`, `crossterm` in `view` and `terminal.rs`, and `spec` depends on nothing else in the crate; [`test:separation`](../mise-tasks/test/separation) checks it.
