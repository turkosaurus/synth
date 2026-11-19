local synth = require("synth")
return synth.sequence({
	bpm = 60,
	instruments = { a = synth.instrument({}) },
	steps = { { note = 69, instrument = "nope" } },
})
