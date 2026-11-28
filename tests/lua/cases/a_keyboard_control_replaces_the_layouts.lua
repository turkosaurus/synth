local synth = require("synth")
local patch = synth.instrument({
	oscillator = synth.osc({ waveforms = { synth.wave.triangle } }),
	envelope = synth.envelope({}),
	keyboard = synth.input({ layout = synth.input.qwerty, octave_up = { "]" } }),
})
-- the shared layout is copied, not changed
assert(synth.input.qwerty.octave_up[1] == "=")
return patch
