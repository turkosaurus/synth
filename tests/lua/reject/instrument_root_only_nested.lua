-- expect: instruments.lead: `filter` applies only to the patch's root; set it on the sequence (instrument made at {file}:7)
local synth = require("synth")

return synth.sequence({
	instrument = "lead",
	instruments = {
		lead = synth.instrument({
			filter = synth.filter.lowpass({ cutoff = 800 }),
		}),
	},
	steps = { { note = 60 } },
})
