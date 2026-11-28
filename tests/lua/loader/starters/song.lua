-- every starter `synth custom` writes, in use together
local synth = require("synth")

return synth.sequence({
	bpm = 120,
	instruments = { lead = require("instruments.example") },
	instrument = "lead",
	filter = require("effects.example"),
	input = { keyboard = require("keyboards.example") },
	tuning = require("tunings.example"),
	patterns = { intro = require("patterns.example") },
	chain = { "intro" },
})
