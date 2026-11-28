-- expect: :4: input.grid: unknown field 'offest'
local synth = require("synth")
return synth.instrument({
	keyboard = synth.input({ layout = synth.input.grid({ offest = 7 }) }),
})
