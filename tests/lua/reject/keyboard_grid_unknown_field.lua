-- expect: :4: keyboard.grid: unknown field 'offest'
local synth = require("synth")
return synth.instrument({
	input = { keyboard = synth.input.keyboard({ layout = synth.input.keyboard.grid({ offest = 7 }) }) },
})
