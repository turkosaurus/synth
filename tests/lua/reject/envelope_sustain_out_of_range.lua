local synth = require("synth")
return synth.instrument({
	envelope = synth.envelope({ sustain = 1.5 }),
})
