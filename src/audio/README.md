# audio

| file | purpose |
| --- | --- |
| [`render.rs`](./render.rs) | `Player`: what the callback runs; never allocates, locks, or blocks |
| [`device.rs`](./device.rs) | the only place `cpal` appears; opens and streams |
| [`fx`](./fx) | per-sample processors, `core` only |

```mermaid
sequenceDiagram
    box ui thread
        participant keyboard as input::keyboard
        participant tx as rtrb::Producer
    end
    box audio callback
        participant rx as rtrb::Consumer
        participant player as render::Player
        participant fx
    end
    participant device

    note over tx,rx: one ring, split by session.rs
    keyboard->>tx: Event
    tx-->>rx: lock-free push
    device->>player: fill buffer
    player->>rx: pop due events
    note over player: voices: phase, wave, envelope
    player->>fx: summed sample
    fx->>device: filtered sample
```

A sequenced patch has no ring: `Player` reads events from its `Timeline` on the sample clock instead.
