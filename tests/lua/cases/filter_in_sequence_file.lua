local sequence = require("synth.sequence")
local instrument = require("synth.instrument")
local filter = require("synth.filter")
return sequence({
	bpm = 60,
	instruments = { a = instrument({}) },
	steps = { { note = 69, instrument = "a" } },
	filter = filter.highpass({ cutoff = 200 }),
})
