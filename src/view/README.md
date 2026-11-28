# view

Presentation only: draws data it is handed, does no work of its own.

| file | purpose |
| --- | --- |
| [`tui.rs`](./tui.rs) | drives a `Session` and draws it |
| [`meter.rs`](./meter.rs) | level meter ballistics |
| [`scope.rs`](./scope.rs) | oscilloscope trigger: the newest frames of a source, held still on a rising zero crossing |
| [`spectrum.rs`](./spectrum.rs) | live spectrum: FFT of the scope's source, on a log frequency axis |
| [`report.rs`](./report.rs) | printed reports: patch, config, environment |
| [`table.rs`](./table.rs) | table formatting for reports |
| [`prompt.rs`](./prompt.rs) | overwrite question for edited built-ins |
| [`diff.rs`](./diff.rs) | line diff shown by the prompt |
| [`log.rs`](./log.rs) | a JSON log event as a coloured line for `synth log` |

```mermaid
flowchart LR
    Session --> tui --> meter & scope & spectrum
    Patch --> report --> table
    prompt --> diff
```
