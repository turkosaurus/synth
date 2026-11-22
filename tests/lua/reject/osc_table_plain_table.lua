-- expect: oscillator: expected an osc, got a plain table
local synth = require("synth")
return synth.instrument({ oscillator = { waves = { synth.wave.sine } } })
