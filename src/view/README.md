# view

Presentation only: draws data it is handed, does no work of its own.

| file | purpose |
| --- | --- |
| [`tui.rs`](./tui.rs) | drives a `Session` and draws it |
| [`meter.rs`](./meter.rs) | level meter ballistics |
| [`spectrum.rs`](./spectrum.rs) | live spectrum: FFT of the output tee, on a log frequency axis |
| [`report.rs`](./report.rs) | printed reports: patch, config, environment |
| [`table.rs`](./table.rs) | table formatting for reports |
| [`prompt.rs`](./prompt.rs) | overwrite question for edited built-ins |
| [`diff.rs`](./diff.rs) | line diff shown by the prompt |

```mermaid
flowchart LR
    Session --> tui --> meter
    tui --> spectrum
    Patch --> report --> table
    prompt --> diff
```
