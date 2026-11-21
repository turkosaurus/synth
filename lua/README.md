# lua

Lua defines not only configuration, but provides functions to the Rust audio engine that define waveforms, slopes, envelopes, the instruments they compose into, and the patterns that may be chained for automated note sequencing.


| file | purpose |
| --- | --- |
| [`synth.lua`](./synth.lua) | `require("synth")`: every module below in one table |
| [`construct.lua`](./construct.lua) | the constructors' shared helper: caller's fields, deep-copied defaults, stamp; not in `require("synth")` |
| [`slope.lua`](./slope.lua) | envelope curves: shapes from 0 to 1, scaled per stage |
| [`wave.lua`](./wave.lua) | waveform shapes |
| [`envelope.lua`](./envelope.lua) | envelope generator |
| [`tuning.lua`](./tuning.lua) | tunings: MIDI note to Hz, equal temperament by default |
| [`instrument.lua`](./instrument.lua) | default instrument |
| [`keyboard.lua`](./keyboard.lua) | mapping character keyboards to MIDI |
| [`filter.lua`](./filter.lua) | filters for the summed output |
| [`sequence.lua`](./sequence.lua) | sequencer blocks composed with chains |
| [`default.lua`](./default.lua) | what plays when no file is given |
| [`example.lua`](./example.lua) | test validator and example |
| [`config.lua`](./config.lua) | default system config bootstrapped by `synth config` |
| [`patches/`](./patches) | example/bootstrap patches written by `synth patches` |

