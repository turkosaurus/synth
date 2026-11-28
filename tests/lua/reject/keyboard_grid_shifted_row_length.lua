-- expect: :7: keyboard.grid: template row 1 and its shifted form differ in length
-- derived rows cannot mismatch; a template written by hand still can
local synth = require("synth")
return synth.instrument({
	input = {
		keyboard = synth.input.keyboard({
			layout = synth.input.keyboard.grid({
				template = { anchor = "a", rows = { "asdfghjkl" }, shifted = { "ASD" } },
			}),
		}),
	},
})
