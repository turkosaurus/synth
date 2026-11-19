local synth = require("synth")
return synth.instrument({
	keyboard = synth.keyboard({
		layout = { notes = { ["="] = 60 }, octave_up = { "=" }, octave_down = {}, help = {} },
	}),
})
