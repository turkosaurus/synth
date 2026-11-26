-- A low-frequency oscillator: a waveform at a slow rate, moving a parameter
-- through that parameter's `<name>_mod` field. `position_mod` runs one LFO
-- per note, from the start of its cycle; `cutoff_mod` runs one for the
-- whole filter, never restarted.
--
-- Depth has no default here: each destination's constructor supplies its
-- own, since its units differ. A frequency such as cutoff takes `depth_hz`
-- or `depth_octaves`, one of them, and no plain `depth`.
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
---@field depth_hz? number on a frequency: how far either side it moves, in Hz
---@field depth_octaves? number on a frequency: how far either side it moves, in octaves

defaults = {
	wave = wave.sine,
	rate = 1, -- Hz
}

return lfo
