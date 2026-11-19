-- expect: `voice_count` must be a whole number, got 2.5
local synth = require("synth")
return synth.instrument({ voice_count = 2.5 })
