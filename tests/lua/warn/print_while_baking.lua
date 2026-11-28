-- expect: print 7 phase\t0
local synth = require("synth")
local printed = false
local function wave(phase)
	if not printed then
		printed = true
		print("phase", phase)
	end
	return synth.wave.sine(phase)
end
return synth.instrument({ oscillator = synth.osc({ waveforms = { wave } }) })
