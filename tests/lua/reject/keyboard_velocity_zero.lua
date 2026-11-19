-- expect: keyboard made at {file}:3: `velocity` is 0; must be 1 to 127
local synth = require("synth")
return synth.instrument({ keyboard = synth.keyboard({ velocity = 0 }) })
