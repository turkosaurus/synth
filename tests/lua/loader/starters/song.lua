-- every starter `synth patches` writes, in use together
local synth = require("synth")

return synth.sequence({
	bpm = 120,
	instruments = { lead = require("instrument.example") },
	instrument = "lead",
	filter = require("effect.example"),
	keyboard = require("keyboard.example"),
	tuning = require("tuning.example"),
	patterns = { intro = require("pattern.example") },
	chain = { "intro" },
})
