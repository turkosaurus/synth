-- expect: needs `depth_hz` or `depth_octaves`
local synth = require("synth")
return synth.instrument({
	filter = synth.filter.lowpass({ cutoff = 800, cutoff_mod = synth.lfo({ rate = 1 }) }),
})
