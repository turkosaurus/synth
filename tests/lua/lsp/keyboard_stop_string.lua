-- expect: assign-type-mismatch
local synth = require("synth")

return synth.instrument({
	keyboard = synth.input({ stop = "escape" }),
})
