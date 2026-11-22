-- expect: `oscillator` must be a synth.osc.periodic({…}) or synth.osc.table({…}), got a function
local synth = require("synth")
return synth.instrument({ oscillator = synth.wave.saw })
