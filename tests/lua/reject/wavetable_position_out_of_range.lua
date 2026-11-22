-- expect: `position` is 2; must be 0 to 1
local synth = require("synth")
return synth.instrument({
	wavetable = synth.wave.table({ waves = { synth.wave.sine, synth.wave.saw }, position = 2 }),
})
