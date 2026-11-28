-- expect: :6: input.grid: template row 1 and its shifted form differ in length
-- derived rows cannot mismatch; a template written by hand still can
local synth = require("synth")
return synth.instrument({
	keyboard = synth.input({
		layout = synth.input.grid({
			template = { anchor = "a", rows = { "asdfghjkl" }, shifted = { "ASD" } },
		}),
	}),
})
