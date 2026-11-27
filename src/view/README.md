# view

Presentation only: draws data it is handed, does no work of its own.

| file | purpose |
| --- | --- |
| [`tui.rs`](./tui.rs) | drives a `Session` and draws it |
| [`meter.rs`](./meter.rs) | level meter ballistics |
| [`scope.rs`](./scope.rs) | oscilloscope trigger: the newest output, held still on a rising zero crossing |
| [`report.rs`](./report.rs) | printed reports: patch, config, environment |
| [`table.rs`](./table.rs) | table formatting for reports |
| [`prompt.rs`](./prompt.rs) | overwrite question for edited built-ins |
| [`diff.rs`](./diff.rs) | line diff shown by the prompt |

```mermaid
flowchart LR
    Session --> tui --> meter & scope
    Patch --> report --> table
    prompt --> diff
```
