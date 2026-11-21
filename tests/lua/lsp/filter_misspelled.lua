-- expect: undefined-field
local synth = require("synth")

return synth.instrument({
	filter = synth.filter.lowpas({ cutoff = 2000 }),
})
