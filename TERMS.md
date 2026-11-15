# dictionary

## generation

| term | type | definition |
| ---  | ---  | ---        |
| attack | ms | on note start, time to reach peak amplitude |
| decay | ms | after attack: time to reach sustain amplitude |
| sustain | `0.0` - `1.0` | after decay: percentage of max amplitude to sustain |
| release | ms | on note end, time to fall from current level to silence |
| envelope (generator) | `ADSR` | time sections: attack, decay, sustain, release (adds articulation to an oscillator) |
| envelope (follower) | `AR` | attack and release rates, no sections (defines reactivity for a compressor) |
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
| decibel | dB | `20 × log10` of an amplitude ratio; +6 dB is about double, -6 dB about half |

## filters

| term | type | definition |
| ---  | ---  | ---        |
| filter | | processor that changes the level of some frequencies more than others |
| cutoff | hz | where a filter acts: the -3 dB point of a lowpass or highpass, the centre of a bandpass, notch, or bell, the midpoint of a shelf |
| resonance | `0.0` - `1.0` | emphasis around the cutoff; 0 is flat, 1 rings hard without self-oscillating |
| damping | `MIN_DAMPING` - `√2` | resonance as the filter computes it (`k`, or 1/Q); lower rings more |
| gain | dB | boost (+) or cut (-) applied by a bell or shelf |
| rolloff | dB/octave | how fast level falls past the cutoff; 12 for the state-variable filter, 6 for a one-pole |
| passband | | frequencies a filter keeps at full level |
| stopband | | frequencies a filter removes |
| lowpass | | keeps frequencies below the cutoff, rolls off above it |
| highpass | | keeps frequencies above the cutoff, rolls off below it |
| bandpass | | keeps a band around the cutoff, rolls off both sides; peaks at 0 dB |
| notch | | removes a narrow band at the cutoff, keeps the rest |
| bell | | boosts or cuts a band around the cutoff by its gain, flat elsewhere; one band of a parametric EQ |
| low shelf | | boosts or cuts everything below the cutoff by its gain, then levels off; a bass knob |
| high shelf | | boosts or cuts everything above the cutoff by its gain, then levels off; a treble knob |
| one-pole | | simplest filter: one value of memory, 6 dB/octave, no resonance; smooths parameters |
| state-variable filter | `svf` | two integrators in a loop, giving lowpass, bandpass, and highpass from shared state; every kind above is a mix of those, and it stays stable when the cutoff moves every sample |
| smoothing | ms | gliding a parameter to a new value rather than jumping to it |
| zipper noise | | audible stepping when a parameter jumps instead of gliding |
| denormal | | float so close to zero that arithmetic on it is very slow; filter state snaps to 0 below `1e-20` |
| impulse response | `[]f32` | a filter's output for one sample of 1.0 followed by silence |
| frequency response | dB per hz | level change at each frequency; the FFT of the impulse response |

## playing

| term | type | definition |
| ---  | ---  | ---        |
| patch | | baked result of one lua file: instruments, timeline, keymap |
| session | | one live run of a patch on the output device, from open until drop: device, stream, rings, input |
| instrument | | lua-defined sound: one waveform, one envelope, baked |
| note | | one sounding instance of an instrument at a pitch |
| voice | | one slot that can sound one note at a time |
| polyphony | count | how many voices exist; 1 is monophonic |
| gate | | held signal; note sounds while high |
| trigger | | instantaneous event; a fixed-length note follows one |
| note stealing | | ending a sounding note to free its slot |
| retrigger | | striking a pitch already sounding |
| buffer | frames | frames the device asks for per callback |
| deadline | µs | buffer / sample rate; time budget for one callback |
| xrun | | callback missed its deadline; device had no audio |
| load | `0.0` - `inf` | fraction of the deadline used by the worst recent callback |
