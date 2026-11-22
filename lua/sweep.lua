-- A wavetable swept by an LFO: hold a key to hear the timbre move.
local synth = require("synth")

return synth.instrument({
	oscillator = synth.osc({
		waveforms = { synth.wave.sine, synth.wave.saw, synth.wave.square },
		position = 0.5,
		position_mod = synth.lfo({
			wave = synth.wave.triangle,
			rate = 0.5, -- Hz
			depth = 0.5,
		}),
	}),
	envelope = synth.envelope({ attack = 20, release = 400 }),
})
