-- derived rows cannot mismatch; a template written by hand still can
local synth = require("synth")
return synth.instrument({
	keyboard = synth.keyboard({
		layout = synth.keyboard.grid({
			template = { anchor = "a", rows = { "asdfghjkl" }, shifted = { "ASD" } },
		}),
	}),
})
