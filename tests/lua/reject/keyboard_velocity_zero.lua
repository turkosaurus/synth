-- expect: keyboard made at {file}:4: `velocity` is 0; must be 1 to 127
-- expect-variant: Error::OutOfRange
local synth = require("synth")
return synth.instrument({ keyboard = synth.keyboard({ velocity = 0 }) })
