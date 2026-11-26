-- expect: unknown field `depth_hz`; expected one of wave, rate, depth
local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.sine, synth.wave.saw },
		position_mod = synth.lfo({ depth_hz = 200 }),
	}),
})
