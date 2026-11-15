-- Filters for the summed output, as `filter = filter.lowpass({ ... })` at
-- the top of a patch. Rust runs them; these constructors only fill in
-- defaults and tag the kind.
--
--   cutoff     Hz, required
--   resonance  0 (flat) to 1 (rings hard); not on shelves
--   gain       dB; only on bell and shelves
--
-- Unknown fields pass through so Rust can name a misspelling.

local filter = {}

local defaults = {
	lowpass = { resonance = 0 },
	highpass = { resonance = 0 },
	bandpass = { resonance = 0 },
	notch = { resonance = 0 },
	bell = { resonance = 0, gain = 0 },
	low_shelf = { gain = 0 },
	high_shelf = { gain = 0 },
}

-- a fresh table: the caller's table and the defaults are never modified
local function constructor(kind)
	return function(opts)
		local out = { _kind = kind }
		for key, value in pairs(defaults[kind]) do
			out[key] = value
		end
		for key, value in pairs(opts or {}) do
			out[key] = value
		end
		return out
	end
end

for kind in pairs(defaults) do
	filter[kind] = constructor(kind)
end

return filter
