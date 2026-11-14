-- What synth plays when no file is given. Every setting is written out, so
-- this doubles as a template for a new instrument.
local instrument = require("synth.instrument")
local envelope = require("synth.envelope")
local keyboard = require("synth.keyboard")
local slope = require("synth.slope")
local wave = require("synth.wave")

return instrument.new({
	waveform = wave.triangle,
	envelope = envelope.new({
		attackMS = 20,
		attackFunc = slope.linear(),
		decayMS = 80,
		decayFunc = slope.linear(),
		sustainLevel = 0.7, -- 0.0 to 1.0
		releaseMS = 800,
		releaseFunc = slope.exponential(2),
	}),
	voiceCount = 4,
	keyboard = keyboard.new({
		gateMS = 400, -- note length when the terminal cannot report key release
	}),
})
