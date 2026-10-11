#!/usr/bin/env luajit

local wave = require("synth.wave")
local envelope = require("synth.envelope")

local instrument = {}

-- passes opts through for the same reason envelope.new does
function instrument.new(opts)
	opts = opts or {}
	if opts.waveform == nil then
		opts.waveform = wave.triangle
	end
	if opts.envelope == nil then
		opts.envelope = envelope.new()
	end
	-- notes this instrument can sound at once; 1 is mono
	if opts.voiceCount == nil then
		opts.voiceCount = 1
	end
	return opts
end

return instrument
