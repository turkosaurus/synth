# audio

| file | purpose |
| --- | --- |
| [`render.rs`](./render.rs) | `Player`: what the callback runs; never allocates, locks, or blocks |
| [`device.rs`](./device.rs) | the only place `cpal` appears; opens and streams |
| [`fx`](./fx) | per-sample processors, `core` only |

```mermaid
sequenceDiagram
    box ui thread
        participant keyboard
    end
    participant ring as rtrb ring
    box audio callback
        participant player as render::Player
        participant fx
    end
    participant device

    keyboard->>ring: Event
    device->>player: fill buffer
    player->>ring: pop due events
    note over player: voices: phase, wave, envelope
    player->>fx: summed sample
    fx->>device: filtered sample
```

A sequenced patch has no ring: `Player` reads events from its `Timeline` on the sample clock instead.
