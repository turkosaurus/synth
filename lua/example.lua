#!/usr/bin/env luajit

local voice = require("synth.voice")
local osc = require("synth.osc")

return voice.new({
	waveform = osc.waveform.triangle,
	envelope = osc.envelope.new({
		attackMS = 10,
		decayMS = 120,
		sustainLevel = 0.7,
		releaseMS = 250,
	}),
})
