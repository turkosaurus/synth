-- expect: instruments.lead: `tuning` applies only to the patch's root; set it on the sequence (instrument made at {file}:7)
local synth = require("synth")

return synth.sequence({
	instrument = "lead",
	instruments = {
		lead = synth.instrument({
			tuning = synth.tuning.equal({ reference_hz = 432 }),
		}),
	},
	steps = { { note = 60 } },
})
