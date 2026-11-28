local instrument = require("synth.instrument")
return instrument({
	oscillator = require("synth.osc")({ waveforms = {
		function(phase)
			return 2 * phase - 1
		end,
	} }),
})
