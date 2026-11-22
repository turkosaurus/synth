-- Oscillators for an instrument, as `oscillator = osc.periodic({ ... })`.
-- Rust bakes them; these constructors only fill in defaults and tag the kind.
--
-- Unknown fields pass through so Rust can name a misspelling.

---@class synth.Osc

---@class synth.OscPeriodic
---@field wave? synth.Waveform one cycle, fixed for as long as a note sounds

-- Waveforms baked one frame each and blended by `position`: n waves make
-- n - 1 transitions, spaced evenly over 0 to 1.
---@class synth.OscTable
---@field waves synth.Waveform[] one frame each, in order
---@field slopes? synth.Curve[] transition i uses slopes[(i - 1) % #slopes + 1]; none is linear
---@field position? number 0 to 1: where between the first and last wave a note sounds
---@field position_mod? synth.Lfo moves `position` while notes sound

-- built in the loop at the end, so declared here for editors
---@class synth.osc
---@field periodic fun(opts?: synth.OscPeriodic): synth.Osc
---@field table fun(opts: synth.OscTable): synth.Osc
local osc = {}

local wave = require("synth.wave")
local construct = require("synth.construct")

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

local defaults = {
	periodic = { wave = wave.triangle },
	table = { slopes = {}, position = 0 },
}

local function constructor(kind)
	-- `_kind` is the oscillator Rust bakes; the stamp marks what built the
	-- table, as every constructor's does
	local fill = { _kind = kind }
	for key, value in pairs(defaults[kind]) do
		fill[key] = value
	end
	return function(opts)
		return construct("osc", site(), opts, fill)
	end
end

for kind in pairs(defaults) do
	osc[kind] = constructor(kind)
end

return osc
