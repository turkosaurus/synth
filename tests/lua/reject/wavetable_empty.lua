-- expect: `waves` is empty; give at least one waveform
local synth = require("synth")
return synth.instrument({ waveform = synth.wave.table({ waves = {} }) })
