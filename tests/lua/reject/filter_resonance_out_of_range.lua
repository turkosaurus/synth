local synth = require("synth")
return synth.instrument({ filter = synth.filter.lowpass({ cutoff = 800, resonance = 1.5 }) })
