-- expect: keyboard.layout plays note 60 on '=', which keyboard.layout.octave_up also binds
local synth = require("synth")
return synth.instrument({
	keyboard = synth.keyboard({
		layout = { notes = { ["="] = 60 }, octave_up = { "=" }, octave_down = {}, help = {} },
	}),
})
