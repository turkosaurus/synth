local synth = require("synth")
return synth.instrument({
	envelope = synth.envelope({
		attack = 10,
		attack_curve = function()
			return 1000
		end,
	}),
})
