-- A low-frequency oscillator: a waveform at a slow rate, moving a parameter
-- through that parameter's `<name>_mod` field. One LFO is shared by every
-- voice of the instrument it moves.
local wave = require("synth.wave")
local construct = require("synth.construct")

local defaults -- defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.lfo
---@overload fun(opts?: synth.Lfo): synth.Lfo
local lfo = setmetatable({}, {
	__call = function(_, opts)
		return construct("lfo", site(), opts, defaults)
	end,
})

---@class synth.Lfo
---@field wave? synth.Waveform its shape, one cycle per period
---@field rate? number cycles per second
---@field depth? number how far either side of the parameter's own value it moves, in that parameter's units

defaults = {
	wave = wave.sine,
	rate = 1, -- Hz
	depth = 0.25,
}

return lfo
