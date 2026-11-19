-- expect: `cutoff` must be positive Hz, got 0
local synth = require("synth")
return synth.instrument({ filter = synth.filter.lowpass({ cutoff = 0 }) })
