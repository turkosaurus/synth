-- expect: keyboard.layout.notes: key 'ab' must be one character
-- a key event carries one character, so "ab" could never be pressed
local synth = require("synth")
return synth.instrument({
	input = {
		keyboard = synth.input.keyboard({
			layout = { notes = { ab = 60 }, octave_up = {}, octave_down = {}, help = {} },
		}),
	},
})
