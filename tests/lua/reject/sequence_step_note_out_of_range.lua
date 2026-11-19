local synth = require("synth")
return synth.sequence({
	instruments = { a = synth.instrument({}) },
	instrument = "a",
	steps = { { note = 60 }, { note = 200 } },
})
