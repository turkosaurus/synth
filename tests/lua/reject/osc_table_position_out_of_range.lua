-- expect: `position` is 2; must be 0 to 1
local synth = require("synth")
return synth.instrument({
	oscillator = synth.osc.table({ waves = { synth.wave.sine, synth.wave.saw }, position = 2 }),
})
