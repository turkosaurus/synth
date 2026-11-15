local slope = require("synth.slope")

local envelope = {}

local defaults = {
	attack = 15, -- ms
	attack_curve = slope.linear(),
	decay = 0, -- ms
	decay_curve = slope.linear(),
	sustain = 1, -- 0.0 to 1.0
	release = 300, -- ms
	release_curve = slope.linear(),
}

-- Pass ops to allow rust validation.
function envelope.new(opts)
	opts = opts or {}
	for key, default in pairs(defaults) do
		if opts[key] == nil then
			opts[key] = default
		end
	end
	return opts
end

return envelope
