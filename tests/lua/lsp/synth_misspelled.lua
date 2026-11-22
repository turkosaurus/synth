-- expect: undefined-field
local synth = require("synth")

return synth.instrument({
	envelope = synth.envelop({ sustain = 0.5 }),
})
