-- expect: `waveforms` is missing or not a list of waveforms
local synth = require("synth")
return synth.instrument({ oscillator = synth.osc({ waveforms = { 3 } }) })
