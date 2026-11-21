-- expect: assign-type-mismatch
local synth = require("synth")

return synth.instrument({
	keyboard = synth.keyboard({ stop = "escape" }),
})
