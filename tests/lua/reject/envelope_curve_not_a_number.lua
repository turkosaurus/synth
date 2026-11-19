-- expect: envelope.release_curve must return a number from 0 to 1, got string
local synth = require("synth")
return synth.instrument({
	envelope = synth.envelope({
		release_curve = function()
			return "slowly"
		end,
	}),
})
