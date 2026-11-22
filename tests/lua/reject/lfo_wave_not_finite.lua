-- expect: lfo wave is NaN at phase 0; must be finite, -1 to 1
local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.sine, synth.wave.saw },
		position_mod = synth.lfo({
			wave = function()
				return 0 / 0
			end,
			rate = 1,
			depth = 0.3,
		}),
	}),
})
