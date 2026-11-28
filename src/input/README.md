# input

Key presses to note events. A layout is Lua's, checked into a `Keymap` by [`script/keymap.rs`](../script/keymap.rs); this module reads keys against it, translated by the view from its own library, and hands each `Event` back to the session, which pushes it into the ring the audio callback reads.

Keys are read on their own thread, never between redraws: the TUI lends the session's `Input` (model and ring producer) to an input thread that pushes each `Event` as soon as its key is read, and sends the draw loop a copied `Glance` of what changed. A MIDI callback thread can own an `Input` the same way.

| file | purpose |
| --- | --- |
| [`keyboard.rs`](./keyboard.rs) | `Keyboard`: key to `Event`, octave shift, control keys, gating |

```mermaid
flowchart LR
    layout["synth.input.keyboard (Lua)"] --> keymap[script::keymap] --> Keymap
    key[terminal key] --> thread[input thread] -- Stroke --> session --> Keyboard
    Keymap --> Keyboard
    Keyboard -- Event --> session -- Cmd --> ring[(rtrb ring)] --> audio
    Keyboard -- Outcome --> session
```

## layouts

[`lua/input/keyboard.lua`](../../lua/input/keyboard.lua) ships `qwerty`, `qwertz`, `azerty`, `dvorak` and `colemak`. Each lines its rows up on the same physical keys, so one position plays the same note in every layout, and each brings its own octave and help keys so none of them takes a note key. A keyboard may replace those keys, as [`custom/keyboards/example.lua`](../../lua/custom/keyboards/example.lua) shows.

## gating

How a note ends depends on whether the terminal reports key release. `synth doctor` says which mode it found.

| mode | when | note ends |
| --- | --- | --- |
| `Held` | release reported: Windows, or a terminal with the kitty keyboard protocol | on release |
| `Fixed` | no release reported | after `gate` ms; a press within `autorepeat_threshold` ms of the key's last press is OS autorepeat and ignored, so a held key sounds once, then once more at the OS repeat delay |
