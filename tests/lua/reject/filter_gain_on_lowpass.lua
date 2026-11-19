-- gain belongs to the shelf and bell filters
local synth = require("synth")
return synth.instrument({ filter = synth.filter.lowpass({ cutoff = 800, gain = 3 }) })
