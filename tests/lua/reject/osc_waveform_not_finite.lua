-- expect: waveform is NaN at phase 0; must be finite, -1 to 1
local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({ waveforms = {
		function()
			return 0 / 0
		end,
	} }),
})
