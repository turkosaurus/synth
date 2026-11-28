local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.saw },
		slopes = { synth.slope.linear() },
		position = 0.5,
	}),
})
