-- expect: shape 8 waveform peaks at only 0.200
-- expect: range 11 cutoff 5 Hz is below the 10 Hz floor and plays as 10 Hz
local synth = require("synth")
local quiet = function(phase)
	return 0.2 * math.sin(phase * 2 * math.pi)
end
return synth.sequence({
	instruments = { soft = synth.instrument({ oscillator = synth.osc({ waveforms = { quiet } }) }) },
	instrument = "soft",
	steps = { { note = 60 } },
	filter = synth.filter.lowpass({ cutoff = 5 }),
})
