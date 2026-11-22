-- expect: wave.table slopes[1] returned a function, not a number
local synth = require("synth")
return synth.instrument({
	waveform = synth.wave.table({
		waves = { synth.wave.sine, synth.wave.saw },
		slopes = { synth.slope.exponential },
	}),
})
