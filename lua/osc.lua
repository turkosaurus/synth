-- An instrument's oscillator: one waveform is a fixed cycle, and several are
-- frames blended by `position`. Rust bakes each waveform at load.
local wave = require("synth.wave")
local construct = require("synth.construct")

local defaults -- defined below

-- `position_mod`'s depth when its LFO gives none
local position_mod_depth = 0.25

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.osc
---@overload fun(opts?: synth.Osc): synth.Osc
local osc = setmetatable({}, {
	__call = function(_, opts)
		local out = construct("osc", site(), opts, defaults)
		local lfo = out.position_mod
		if type(lfo) == "table" and lfo.depth == nil then
			-- filled on a copy: the patch's own table is left alone
			local filled = { depth = position_mod_depth }
			for key, value in pairs(lfo) do
				filled[key] = value
			end
			out.position_mod = setmetatable(filled, getmetatable(lfo))
		end
		return out
	end,
})

-- n waveforms make n - 1 transitions, spaced evenly over `position` 0 to 1.
-- With one waveform, `slopes`, `position`, and `position_mod` do nothing.
---@class synth.Osc
---@field waveforms? synth.Waveform[] one frame each, in order
---@field slopes? synth.Curve[] transition i uses slopes[(i - 1) % #slopes + 1]; none is linear
---@field position? number 0 to 1: where between the first and last waveform a note sounds
---@field position_mod? synth.Lfo moves `position` while notes sound; `depth` defaults to 0.25

defaults = {
	waveforms = { wave.triangle },
	slopes = {},
	position = 0,
}

return osc
