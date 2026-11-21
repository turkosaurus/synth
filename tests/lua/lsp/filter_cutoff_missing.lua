-- expect: missing-fields
local synth = require("synth")

return synth.instrument({
	filter = synth.filter.lowpass({ resonance = 0.2 }),
})
