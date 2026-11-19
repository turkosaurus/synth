-- expect: envelope: expected an envelope, got a plain table; build it with synth.envelope({…})
local synth = require("synth")
return synth.instrument({ envelope = { attack = 5 } })
