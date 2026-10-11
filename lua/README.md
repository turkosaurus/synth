# lua

Lua defines not only configuration, but provides functions to the Rust audio engine that define waveforms, slopes, envelopes, the instruments they compose into, and the patterns that may be chained for automated note sequencing.


| file | purpose |
| --- | --- |
| [`slope.lua`](./slope.lua) | linear and exponential ramps |
| [`wave.lua`](./wave.lua) | waveform shapes |
| [`envelope.lua`](./envelope.lua) | envelope generator |
| [`tuning.lua`](./tuning.lua) | MIDI note to Hz |
| [`instrument.lua`](./instrument.lua) | default mono instrument |
| [`keyboard.lua`](./keyboard.lua) | mapping character keyboards to MIDI |
| [`sequence.lua`](./sequence.lua) | sequencer blocks composed with chains |
| [`example.lua`](./example.lua) | test validator and example |

