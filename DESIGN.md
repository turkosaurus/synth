# design

_Lua defined, Rust run_

## architecture 

Except where necessary to preserve a non-blocking, non-allocating audio pipeline, care is taken to implement [the ELM architecture](https://guide.elm-lang.org/architecture/), separating data, mutation, and presentation.

Goals:
- fast: bad fidelity sounds cool, bad timing sounds *wrong*
- extendable: anything not required to support speed should be extendable with Lua 

## backend

### lua

- oscillator/generator: mathematical slopes and waveforms defined by Lua functions
- envelope: same slopes may define an ADSR amp envelope 
- sequencer: composed voices may be sequenced and chained 

### rust

#### baking

Lua waveforms are built as a single oscillation across `TABLE_SIZE` samples, expected to be sub-sonic, such that one can produce a particular pitch by selecting 1-in-10 samples, then produce a pitch one octave higher by selecting 1-in-20 samples.

The pitch produced by the table is delivered to the audio backend in three sections:
- attack+decay (fixed-length)
- sustain (variable)[^sustain_looping]

[^sustain_looping]: Stored as a single oscillation, at pitch, looped while held or for duration of fixed length, or until interrupted)
- release (fixed-length)

Baking is sample-rate dependent, and comes from the system, so must be handled by the Rust side.

#### triggering

Listeners detect MIDI codes, whether from character keyboard translation in a terminal, or from a USB device, and produced patches are handed to a real-time ring buffer.

#### dispatching

Non-blocking, pure Rust audio pipeline that consumes a real-time ring buffer.

## frontend

### terminal

Character keyboard and TUI can provide the most minimal input and output, respectively. 
