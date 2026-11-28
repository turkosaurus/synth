-- expect: assign-type-mismatch
local synth = require("synth")

return synth.instrument({
	input = { keyboard = synth.input.keyboard({ stop = "escape" }) },
})
