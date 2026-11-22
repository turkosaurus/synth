-- An instrument's oscillator: one waveform is a fixed cycle, and several are
-- frames blended by `position`. Rust bakes each waveform at load.
local wave = require("synth.wave")
local construct = require("synth.construct")

local defaults -- defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.osc
---@overload fun(opts?: synth.Osc): synth.Osc
local osc = setmetatable({}, {
	__call = function(_, opts)
		return construct("osc", site(), opts, defaults)
	end,
})

-- n waveforms make n - 1 transitions, spaced evenly over `position` 0 to 1.
-- With one waveform, `slopes`, `position`, and `position_mod` do nothing.
---@class synth.Osc
---@field waveforms? synth.Waveform[] one frame each, in order
---@field slopes? synth.Curve[] transition i uses slopes[(i - 1) % #slopes + 1]; none is linear
---@field position? number 0 to 1: where between the first and last waveform a note sounds
---@field position_mod? synth.Lfo moves `position` while notes sound

defaults = {
	waveforms = { wave.triangle },
	slopes = {},
	position = 0,
}

return osc
