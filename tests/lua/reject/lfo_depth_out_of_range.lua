-- expect: `depth` is 2; must be 0 to 1
local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.sine, synth.wave.saw },
		position_mod = synth.lfo({ depth = 2 }),
	}),
})
