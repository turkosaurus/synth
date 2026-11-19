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
        participant rx as Feed::Live<br/>rtrb::Consumer
        participant timeline as Feed::Timeline
        participant player as render::Player
        participant fx
    end
    participant device

    note over tx,rx: one ring, split by session.rs
    device->>player: fill buffer
    alt live
        keyboard->>tx: Event
        tx-->>rx: lock-free push
        rx->>player: Event
    else sequenced
        timeline->>player: Event, due on the sample clock
    end
    note over player: voices: phase, wave, envelope
    player->>fx: summed sample
    fx->>device: filtered sample
```
