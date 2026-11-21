-- expect: assign-type-mismatch
local synth = require("synth")

return synth.instrument({
	envelope = synth.envelope({ sustain = "loud" }),
})
