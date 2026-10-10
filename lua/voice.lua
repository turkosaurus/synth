#!/usr/bin/env luajit

local p = require("synth.osc")

local voice = {}

function voice.new(opts)
	opts = opts or {}
	return {
		waveform = opts.waveform or p.waveform.triangle,
		envelope = opts.envelope or p.envelope.new(),
	}
end

return voice
