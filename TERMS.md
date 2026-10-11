# dictionary

## generation

| term | type | definition |
| ---  | ---  | ---        |
| attack | ms | on note start, time to reach peak amplitude |
| decay | ms | after attack: time to reach sustain amplitude |
| sustain | `0.0` - `1.0` | after decay: percentage of max amplitude to sustain |
| release | ms | on note end, time to fall from current level to silence |
| envelope | `ADSR` | time sections: attack, decay, sustain, release |
| slope | `lua:func` | points along a sloped curve between two points |
| waveform | `lua:func` | phase (`0.0` to `1.0`) to amplitude (`-1.0` to `1.0`) |
| wavetable | `[]f32` | one cycle of a waveform, baked to `TABLE_SIZE` points |
| phase | `0.0` - `1.0` | position in oscillation cycle |
| phase increment | `0.0` - `1.0` | phase advanced per frame: frequency / sample rate |
| stride | entries | table entries advanced per frame; double to octave up |
| interpolation |  | blend of the two entries either side of a fractional index |
| seam | `0.0` - `2.0` | step between last and first entry of a cycle; clicks once per cycle |

## signals

| term | type | definition |
| ---  | ---  | ---        |
| sample | `-1.0` - `1.0` | one amplitude value, one channel |
| frame | | one sample per channel; stereo is two |
| sample rate | hz | frames per second, set by the device |
| nyquist | hz | half the sample rate; highest frequency representable |
| aliasing | | harmonics above nyquist folding back as false low frequencies |
| headroom | `0.0` - `1.0` | margin between peak and clipping at 1.0 |
| dc offset | `-1.0` - `1.0` | oscillation mean; rests the speaker cone off-centre |

## playing

| term | type | definition |
| ---  | ---  | ---        |
| patch | | baked result of one lua file: voices, timeline, keymap |
| voice | | lua-defined timbre: one waveform, one envelope, baked |
| note | | one sounding instance of a voice at a pitch |
| gate | | held signal; note sounds while high |
| trigger | | instantaneous event; a fixed-length note follows one |
| note stealing | | ending a sounding note to free its slot |
| retrigger | | striking a pitch already sounding |
| buffer | frames | frames the device asks for per callback |
| deadline | µs | buffer / sample rate; time budget for one callback |
| xrun | | callback missed its deadline; device had no audio |
| load | `0.0` - `inf` | fraction of the deadline used by the worst recent callback |
