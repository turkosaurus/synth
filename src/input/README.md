# input

Key presses to note events. A layout is Lua's, checked into a `Keymap` by [`script/keymap.rs`](../script/keymap.rs); this module reads terminal keys against it and pushes each `Event` into the ring the audio callback reads.

| file | purpose |
| --- | --- |
| [`keyboard.rs`](./keyboard.rs) | `Keyboard`: key to `Event`, octave shift, control keys, gating |

```mermaid
flowchart LR
    layout["synth.keyboard (Lua)"] --> keymap[script::keymap] --> Keymap
    key[terminal key] --> Keyboard
    Keymap --> Keyboard
    Keyboard -- Event --> ring[(rtrb ring)] --> audio
    Keyboard -- Outcome --> view
```

## layouts

[`lua/keyboard.lua`](../../lua/keyboard.lua) ships `qwerty`, `qwertz`, `azerty`, `dvorak` and `colemak`. Each lines its rows up on the same physical keys, so one position plays the same note in every layout, and each brings its own octave and help keys so none of them takes a note key. A keyboard may replace those keys, as [`patches/keyboard/example.lua`](../../lua/patches/keyboard/example.lua) shows.

## gating

How a note ends depends on whether the terminal reports key release. `synth doctor` says which mode it found.

| mode | when | note ends |
| --- | --- | --- |
| `Held` | release reported: Windows, or a terminal with the kitty keyboard protocol | on release |
| `Fixed` | no release reported | after `gate` ms; a repeat press within `repeat_window` ms is OS autorepeat and ignored |
