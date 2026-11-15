-- What synth plays when no file is given. Every setting is written out, so
-- this doubles as a template for a new instrument.
local synth = require("synth")

return synth.instrument.new({
	waveform = synth.wave.triangle,
	envelope = synth.envelope.new({
		attack = 20, -- ms
		attack_curve = synth.slope.linear(),
		decay = 80, -- ms
		decay_curve = synth.slope.linear(),
		sustain = 0.7, -- 0.0 to 1.0
		release = 800, -- ms
		release_curve = synth.slope.exponential(2),
	}),
	voiceCount = 4,
	keyboard = synth.keyboard.new({
		gateMS = 400, -- note length when the terminal cannot report key release
	}),
})
