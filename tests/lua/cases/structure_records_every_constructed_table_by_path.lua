local synth = require("synth")
local tri =
	synth.instrument({ oscillator = synth.osc({ waveforms = { synth.wave.triangle } }), envelope = synth.envelope({}) })
return synth.sequence({ instruments = { tri = tri }, instrument = "tri", steps = { { note = 60 } } })
