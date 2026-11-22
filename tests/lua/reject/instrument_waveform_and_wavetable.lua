-- expect: sets both `waveform` and `wavetable`; set one
local synth = require("synth")
return synth.instrument({
	waveform = synth.wave.periodic(synth.wave.saw),
	wavetable = synth.wave.table({ waves = { synth.wave.sine } }),
})
