-- expect: osc slopes[1] returned a function, not a number
local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.sine, synth.wave.saw },
		slopes = { synth.slope.exponential },
	}),
})
