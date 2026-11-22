-- expect: waveform: expected a wave.table, got a plain table; build it with synth.wave.table({…})
local synth = require("synth")
return synth.instrument({ waveform = { waves = { synth.wave.sine } } })
