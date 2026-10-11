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
	return opts
end

return instrument
