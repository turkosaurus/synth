local synth = require("synth")
return synth.instrument({
	envelope = synth.envelope({
		attack = 2000,
		attack_curve = synth.slope.exponential(3),
	}),
})
