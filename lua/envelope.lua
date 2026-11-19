-- Attack, Decay, Sustain, Release (ADSR) envelope generator,
-- defined as a table of parameters and first-class functions,
-- dispatched to Rust to produce pitched audio.
local slope = require("synth.slope")

local new -- the constructor, defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.envelope
---@overload fun(opts?: table): table
local envelope = setmetatable({}, {
	__call = function(_, opts)
		return setmetatable(new(opts), { kind = "envelope", at = site() })
	end,
})

local defaults = {
	attack = 15, -- ms
	attack_curve = slope.linear(),
	decay = 0, -- ms
	decay_curve = slope.linear(),
	sustain = 1, -- 0.0 to 1.0
	release = 300, -- ms
	release_curve = slope.linear(),
}

-- A new table: the caller's fields, then defaults for any left out. The
-- caller's table is left alone. Unknown fields are kept, so a misspelling
-- reaches Rust and gets named instead of silently taking a default.
new = function(opts)
	local out = {}
	for key, value in pairs(opts or {}) do
		out[key] = value
	end
	for key, default in pairs(defaults) do
		if out[key] == nil then
			out[key] = default
		end
	end
	return out
end

return envelope
