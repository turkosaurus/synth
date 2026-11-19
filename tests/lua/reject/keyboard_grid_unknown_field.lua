-- expect: :4: keyboard.grid: unknown field 'offest'
local synth = require("synth")
return synth.instrument({
	keyboard = synth.keyboard({ layout = synth.keyboard.grid({ offest = 7 }) }),
})
