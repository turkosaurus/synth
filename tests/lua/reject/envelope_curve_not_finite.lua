-- expect: envelope.attack_curve returned NaN at t = 0; must be a number from 0 to 1
local synth = require("synth")
return synth.instrument({
	envelope = synth.envelope({
		attack_curve = function(t)
			return t / t
		end,
	}),
})
