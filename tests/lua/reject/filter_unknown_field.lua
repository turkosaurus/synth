-- expect: unknown field `cutof`; expected one of _kind, cutoff, cutoff_mod, resonance
local synth = require("synth")
return synth.instrument({ filter = synth.filter.lowpass({ cutoff = 800, cutof = 900 }) })
