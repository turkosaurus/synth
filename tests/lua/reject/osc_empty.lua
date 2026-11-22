-- expect: `waveforms` is empty; give at least one
local synth = require("synth")
return synth.instrument({ oscillator = synth.osc({ waveforms = {} }) })
