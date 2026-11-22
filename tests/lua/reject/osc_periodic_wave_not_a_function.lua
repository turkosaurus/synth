-- expect: `wave` must be a function, got 3
local synth = require("synth")
return synth.instrument({ oscillator = synth.osc.periodic({ wave = 3 }) })
