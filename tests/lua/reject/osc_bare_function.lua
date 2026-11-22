-- expect: `oscillator` must be a synth.osc({…}), got a function
local synth = require("synth")
return synth.instrument({ oscillator = synth.wave.saw })
