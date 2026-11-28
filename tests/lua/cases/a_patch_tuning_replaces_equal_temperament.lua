local synth = require("synth")
return synth.instrument({
	tuning = synth.tuning.equal({ reference_hz = 432, tones = 19 }),
})
