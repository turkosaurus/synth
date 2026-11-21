-- expect: assign-type-mismatch
local synth = require("synth")

return synth.instrument({
	tuning = { reference_hz = 432 },
})
