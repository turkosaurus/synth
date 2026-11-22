-- expect: wavetable: expected a wave.table, got a plain table; build it with synth.wave.table({…})
local synth = require("synth")
return synth.instrument({ wavetable = { waves = { synth.wave.sine } } })
