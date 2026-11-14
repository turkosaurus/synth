#!/usr/bin/env luajit

local slope = require("synth.slope")

local envelope = {}

local envelopeDefaults = {
	attackMS = 15,
	attackFunc = slope.linear(),
	decayMS = 0,
	decayFunc = slope.linear(),
	sustainLevel = 1, -- 0.0 to 1.0
	releaseMS = 300,
	releaseFunc = slope.linear(),
}

-- Pass ops to allow rust validation.
function envelope.new(opts)
	opts = opts or {}
	for key, default in pairs(envelopeDefaults) do
		if opts[key] == nil then
			opts[key] = default
		end
	end
	return opts
end

return envelope
