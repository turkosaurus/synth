-- expect: give `depth_hz` or `depth_octaves`, not both
local synth = require("synth")
return synth.instrument({
	filter = synth.filter.lowpass({
		cutoff = 800,
		cutoff_mod = synth.lfo({ rate = 1, depth_hz = 200, depth_octaves = 1 }),
	}),
})
