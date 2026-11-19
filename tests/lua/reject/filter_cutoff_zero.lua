local synth = require("synth")
return synth.instrument({ filter = synth.filter.lowpass({ cutoff = 0 }) })
