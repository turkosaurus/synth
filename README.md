# synth
[![test](https://github.com/turkosaurus/synth/actions/workflows/test.yml/badge.svg)](https://github.com/turkosaurus/synth/actions/workflows/test.yml)
![GitHub Release](https://img.shields.io/github/v/release/turkosaurus/synth)

_Lua-defined, Rust-driven synthesizer and sequencer_

## install

Linux x86_64 and macOS arm64:
```sh
curl -fsSL https://raw.githubusercontent.com/turkosaurus/synth/main/install.sh | bash
```

[`install.sh`](./install.sh) downloads and verifies the latest stable release, installing it to `$XDG_BIN_HOME`, or `~/.local/bin`.

> [!NOTE]
> `update` and `uninstall` act on the binary being run; defer to package manager if installed through one.

## requirements

### linux
```sh
sudo apt install libasound2-dev pkg-config build-essential
```

Run TUI:
```sh
mise run tui
```

## architecture 

Except where necessary to preserve a non-blocking, non-allocating audio pipeline, care is taken to implement [the ELM architecture](https://guide.elm-lang.org/architecture/), separating data, mutation, and presentation.

That exception is held by the module layout rather than by discipline, and `mise run test:local` enforces it: `mlua` may appear only in [`script.rs`](./src/script.rs) and under [`script/`](./src/script/), and `cpal` only in [`device.rs`](./src/audio/device.rs). [`render.rs`](./src/audio/render.rs) is therefore exactly what the audio callback executes, with no language runtime and no device within reach.

Goals:
- fast: bad fidelity sounds cool, bad timing sounds *wrong*
- extendable: anything not required to support speed should be extendable with Lua 

Views are assembled from data, and presentation layers never do additional work.

### diagram

```mermaid
flowchart LR
    lua[(lua)] --> script --> spec[(spec)] --> audio
    input -- events --> audio
    audio --> output([OS audio])
    session -. wires .-> input & audio
    view -. drives .-> session
```

Each part has its own README: [`lua`](./lua), [`script`](./src/script), [`spec`](./src/spec), [`audio`](./src/audio), [`view`](./src/view), or [`src`](./src) for the whole.

## backend

### lua

- oscillator/generator: mathematical slopes and waveforms defined by Lua functions
- envelope: same slopes may define an ADSR amp envelope 
- sequencer: composed instruments may be sequenced and chained 

### rust

#### baking

Lua waveforms are built as a single oscillation across `TABLE_SIZE` samples, expected to be sub-sonic, such that one can produce a particular pitch by selecting 1-in-10 samples, then produce a pitch one octave higher by selecting 1-in-20 samples. Phase is usually fractional, so playback interpolates between neighbouring entries.

Lua states both shapes as functions Rust samples. A waveform is a function of phase, so Rust calls it once per point across the table. An envelope curve is a function of t, from 0 to 1 across one stage, so Rust calls it at the same 1025 points whatever the stage's length, and playback stretches it over the stage and scales it from the stage's start level to its end.

2048 points means every note above 23.4 Hz advances at least one entry per output sample, so no playable pitch stretches the shape; at 8 KB it also stays in L1.

An instrument reaches the audio backend as two things: one cycle of waveform, and an amplitude envelope that scales it. The waveform loops at pitch for as long as the note sounds, in every stage.

The envelope is baked in four parts:
- attack (fixed-length)
- decay (fixed-length)
- sustain (a level, variable duration)[^sustain_looping]
- release (fixed-length)

[^sustain_looping]: Sustain is a level, not a buffer. The waveform keeps looping while held, or for a duration of fixed length, or until interrupted; the envelope just stops moving.

Baking does not depend on the sample rate: playback sizes each stage from its length in ms at the device's rate.

#### triggering

Listeners detect MIDI codes, whether from character keyboard translation in a terminal, or from a USB device, and the resulting note events are handed to a real-time ring buffer.

#### dispatching

Non-blocking, pure Rust audio pipeline that consumes a real-time ring buffer.

## frontend

### terminal

Character keyboard and TUI can provide the most minimal input and output, respectively. 

## license

[GPL-3.0-or-later](./LICENSE)
