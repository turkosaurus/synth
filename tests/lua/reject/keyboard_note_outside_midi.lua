-- expect: keyboard.layout.notes['a'] is 128; must be a whole number 0 to 127
-- expect-variant: Error::NoteRange
local synth = require("synth")
return synth.instrument({
	keyboard = synth.keyboard({
		layout = { notes = { a = 128 }, octave_up = {}, octave_down = {}, help = {} },
	}),
})
