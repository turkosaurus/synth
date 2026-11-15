-- What synth plays when no file is given. Every setting is written out, so
-- this doubles as a template for a new instrument.
local synth = require("synth")

return synth.instrument.new({
	waveform = synth.wave.triangle,
	envelope = synth.envelope.new({
		attackMS = 20,
		attackFunc = synth.slope.linear(),
		decayMS = 80,
		decayFunc = synth.slope.linear(),
		sustainLevel = 0.7, -- 0.0 to 1.0
		releaseMS = 800,
		releaseFunc = synth.slope.exponential(2),
	}),
	voiceCount = 4,
	keyboard = synth.keyboard.new({
		gateMS = 400, -- note length when the terminal cannot report key release
	}),
})
