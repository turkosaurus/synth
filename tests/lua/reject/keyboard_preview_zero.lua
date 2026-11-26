-- expect: keyboard made at {file}:3: `preview` is 0; must be above 0
local synth = require("synth")
return synth.instrument({ keyboard = synth.keyboard({ preview = 0 }) })
