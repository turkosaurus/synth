local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.sine, synth.wave.saw, synth.wave.square },
		slopes = { synth.slope.exponential(3) },
		position = 0.25,
	}),
})
