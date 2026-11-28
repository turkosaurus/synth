local synth = require("synth")
local lfo = synth.lfo({ rate = 0.5 })
local osc = synth.osc({
	waveforms = { synth.wave.sine, synth.wave.saw },
	position_mod = lfo,
})
assert(lfo.depth == nil, "osc wrote depth onto the patch's own lfo")
return synth.instrument({ oscillator = osc })
