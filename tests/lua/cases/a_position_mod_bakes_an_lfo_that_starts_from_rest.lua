local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.sine, synth.wave.saw },
		position_mod = synth.lfo({ wave = synth.wave.sine, rate = 0.5, depth = 0.3 }),
	}),
})
