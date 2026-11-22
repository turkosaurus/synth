-- expect: oscillator: expected an osc, got a plain table; build it with synth.osc({…})
local synth = require("synth")
return synth.instrument({ oscillator = { waveforms = { synth.wave.sine } } })
