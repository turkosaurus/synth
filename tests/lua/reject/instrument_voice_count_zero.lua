-- expect: `voice_count` is 0; must be at least 1
local synth = require("synth")
return synth.instrument({ voice_count = 0 })
