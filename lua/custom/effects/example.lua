-- An effect of your own. A patch uses it with
--   filter = require("effects.example")
local synth = require("synth")

return synth.filter.lowpass({
	cutoff = 1200, -- Hz
	resonance = 0.3, -- 0 to 1
})

-- other filters take the same shape:
--   synth.filter.highpass({ cutoff = 200 })
--   synth.filter.bell({ cutoff = 800, gain = 6 }) -- dB
--   synth.filter.low_shelf({ cutoff = 150, gain = -3 })
