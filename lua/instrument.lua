local osc = require("synth.osc")
local envelope = require("synth.envelope")
local construct = require("synth.construct")

local defaults -- defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.instrument
---@overload fun(opts?: synth.Instrument): synth.Instrument
local instrument = setmetatable({}, {
	__call = function(_, opts)
		return construct("instrument", site(), opts, defaults)
	end,
})

-- `input`, `tuning`, and `filter` apply when the instrument is the
-- patch's root, and are rejected on an instrument inside a sequence.
---@class synth.Instrument
---@field oscillator? synth.Osc
---@field envelope? synth.Envelope
---@field voice_count? integer notes it can sound at once; 1 is mono
---@field input? synth.Input
---@field tuning? synth.Tuning
---@field filter? synth.Filter

defaults = {
	oscillator = osc(),
	-- copied for each instrument, so none shares another's envelope
	envelope = envelope(),
	-- notes this instrument can sound at once; 1 is mono
	voice_count = 4,
}

return instrument
