-- An instrument's oscillator: one waveform is a fixed cycle, and several are
-- cycles blended by `position`. Rust bakes each waveform at load.
local wave = require("synth.wave")
local construct = require("synth.construct")

local defaults, lfo_defaults -- defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.osc
---@overload fun(opts?: synth.Osc): synth.Osc
local osc = setmetatable({}, {
	__call = function(_, opts)
		local out = construct("osc", site(), opts, defaults)
		for field, fill in pairs(lfo_defaults) do
			local lfo = out[field]
			if type(lfo) == "table" then
				-- filled on a copy: the patch's own table is left alone
				local filled = {}
				for key, value in pairs(fill) do
					filled[key] = value
				end
				for key, value in pairs(lfo) do
					filled[key] = value
				end
				out[field] = setmetatable(filled, getmetatable(lfo))
			end
		end
		return out
	end,
})

-- n waveforms make n - 1 transitions, spaced evenly over `position` 0 to 1.
-- With one waveform, `slopes`, `position`, and `position_mod` do nothing.
---@class (exact) synth.Osc
---@field waveforms? synth.Waveform[] one cycle each, in order
---@field slopes? synth.Curve[] transition i uses slopes[(i - 1) % #slopes + 1]; none is linear
---@field position? number 0 to 1: where between the first and last waveform a note sounds
---@field position_mod? synth.Lfo moves `position` while notes sound; `depth` defaults to 0.25

defaults = {
	waveforms = { wave.triangle },
	slopes = {},
	position = 0,
}

-- filled into an LFO a field holds, wherever the LFO leaves them out
lfo_defaults = {
	position_mod = { depth = 0.25 },
}

return osc
