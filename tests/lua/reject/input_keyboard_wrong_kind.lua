-- expect: input.keyboard: expected a keyboard, got an envelope made at {file}:3
local synth = require("synth")
return synth.instrument({ input = { keyboard = synth.envelope({}) } })
