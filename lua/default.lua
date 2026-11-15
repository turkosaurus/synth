-- What synth plays when no file is given. Every setting is written out, so
-- this doubles as a template for a new instrument.
local synth = require("synth")

return synth.instrument({
	waveform = synth.wave.triangle,
	envelope = synth.envelope({
		attack = 20, -- ms
		attack_curve = synth.slope.linear(),
		decay = 80, -- ms
		decay_curve = synth.slope.linear(),
		sustain = 0.7, -- 0.0 to 1.0
		release = 800, -- ms
		release_curve = synth.slope.exponential(2),
	}),
	voice_count = 4,
	tuning = synth.tuning.equal({ reference_hz = 440, reference_note = 69, tones = 12 }),
	keyboard = synth.keyboard({
		gate = 400, -- ms; note length when the terminal cannot report key release
	}),
})
