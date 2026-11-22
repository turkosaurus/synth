-- expect: waves[2] is NaN at phase 0.5; must be finite, -1 to 1
local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = {
			synth.wave.sine,
			function(phase)
				return phase < 0.5 and 0 or 0 / 0
			end,
		},
	}),
})
