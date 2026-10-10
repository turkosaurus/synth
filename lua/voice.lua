#!/usr/bin/env luajit

local p = require("synth.osc")

local voice = {}

-- passes opts through for the same reason envelope.new does
function voice.new(opts)
	opts = opts or {}
	if opts.waveform == nil then
		opts.waveform = p.waveform.triangle
	end
	if opts.envelope == nil then
		opts.envelope = p.envelope.new()
	end
	return opts
end

return voice
