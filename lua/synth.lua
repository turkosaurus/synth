-- The library root: one require reaches every module.
-- Each module stays requirable on its own as synth.<name>, so overriding one
-- never means overriding this file too.
return {
	envelope = require("synth.envelope"),
	instrument = require("synth.instrument"),
	keyboard = require("synth.keyboard"),
	sequence = require("synth.sequence"),
	slope = require("synth.slope"),
	tuning = require("synth.tuning"),
	wave = require("synth.wave"),
}
