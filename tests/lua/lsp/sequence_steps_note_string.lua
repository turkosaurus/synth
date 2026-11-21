-- expect: assign-type-mismatch
local synth = require("synth")

return synth.sequence({
	steps = { { note = "A4" } },
})
