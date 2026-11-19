-- expect: `resonance` must be 0 to 1, got 1.5
local synth = require("synth")
return synth.instrument({ filter = synth.filter.lowpass({ cutoff = 800, resonance = 1.5 }) })
