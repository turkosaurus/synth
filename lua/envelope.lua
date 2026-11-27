-- Attack, Decay, Sustain, Release (ADSR) envelope generator,
-- defined as a table of parameters and first-class functions,
-- baked by Rust into an amplitude curve at load.
local slope = require("synth.slope")
local construct = require("synth.construct")

local defaults -- defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.envelope
---@overload fun(opts?: synth.Envelope): synth.Envelope
local envelope = setmetatable({}, {
	__call = function(_, opts)
		return construct("envelope", site(), opts, defaults)
	end,
})

---@class (exact) synth.Envelope
---@field attack? number ms
---@field attack_curve? synth.Curve
---@field decay? number ms
---@field decay_curve? synth.Curve
---@field sustain? number 0.0 to 1.0
---@field release? number ms
---@field release_curve? synth.Curve
defaults = {
	attack = 15, -- ms
	attack_curve = slope.linear(),
	decay = 0, -- ms
	decay_curve = slope.linear(),
	sustain = 1, -- 0.0 to 1.0
	release = 300, -- ms
	release_curve = slope.linear(),
}

return envelope
