-- expect: keyboard.layout plays note 60 on '=', which keyboard.layout.octave_up also binds
-- expect-variant: Error::KeyConflict { what: "layout.octave_up", key: '=', note: 60 }
local synth = require("synth")
return synth.instrument({
	keyboard = synth.keyboard({
		layout = { notes = { ["="] = 60 }, octave_up = { "=" }, octave_down = {}, help = {} },
	}),
})
