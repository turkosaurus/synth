local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.saw },
		position_mod = synth.lfo({}),
	}),
})
