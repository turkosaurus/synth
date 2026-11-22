-- library root, can be overridden
---@class synth
local synth = {
	envelope = require("synth.envelope"),
	filter = require("synth.filter"),
	instrument = require("synth.instrument"),
	keyboard = require("synth.keyboard"),
	lfo = require("synth.lfo"),
	osc = require("synth.osc"),
	sequence = require("synth.sequence"),
	slope = require("synth.slope"),
	tuning = require("synth.tuning"),
	wave = require("synth.wave"),
}

return synth
