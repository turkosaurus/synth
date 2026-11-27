-- Filters for the summed output, as `filter = filter.lowpass({ ... })` at
-- the top of a patch. Rust runs them; these constructors only fill in
-- defaults and tag the kind.
--
--   cutoff     Hz, required
--   resonance  0 (flat) to 1 (rings hard); not on shelves
--   gain       dB; only on bell and shelves
--   cutoff_mod synth.lfo with `depth_hz` or `depth_octaves`; one LFO for
--              the whole filter, free-running from load
--
-- Unknown fields pass through so Rust can name a misspelling.

---@class (exact) synth.Filter
---@field cutoff number Hz
---@field resonance? number 0 (flat) to 1 (rings hard); not on shelves
---@field gain? number dB; only on bell and shelves
---@field cutoff_mod? synth.Lfo moves `cutoff` by `depth_hz` or `depth_octaves`

-- built in the loop at the end, so declared here for editors
---@class synth.filter
---@field lowpass fun(opts: synth.Filter): synth.Filter
---@field highpass fun(opts: synth.Filter): synth.Filter
---@field bandpass fun(opts: synth.Filter): synth.Filter
---@field notch fun(opts: synth.Filter): synth.Filter
---@field bell fun(opts: synth.Filter): synth.Filter
---@field low_shelf fun(opts: synth.Filter): synth.Filter
---@field high_shelf fun(opts: synth.Filter): synth.Filter
local filter = {}

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

local defaults = {
	lowpass = { resonance = 0 },
	highpass = { resonance = 0 },
	bandpass = { resonance = 0 },
	notch = { resonance = 0 },
	bell = { resonance = 0, gain = 0 },
	low_shelf = { gain = 0 },
	high_shelf = { gain = 0 },
}

local construct = require("synth.construct")

local function constructor(kind)
	-- `_kind` is the filter type Rust reads; the stamp marks what built the
	-- table, as every constructor's does
	local fill = { _kind = kind }
	for key, value in pairs(defaults[kind]) do
		fill[key] = value
	end
	return function(opts)
		return construct("filter", site(), opts, fill)
	end
end

for kind in pairs(defaults) do
	filter[kind] = constructor(kind)
end

return filter
