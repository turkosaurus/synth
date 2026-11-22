local wave = require("synth.wave")
local envelope = require("synth.envelope")
local construct = require("synth.construct")

local defaults -- defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.instrument
---@overload fun(opts?: synth.Instrument): synth.Instrument
local instrument = setmetatable({}, {
	__call = function(_, opts)
		-- a patch that sets `wavetable` takes no default `waveform`, so
		-- setting both is always the patch's own doing
		local fill = defaults
		if opts and opts.wavetable ~= nil then
			fill = {}
			for key, value in pairs(defaults) do
				if key ~= "waveform" then
					fill[key] = value
				end
			end
		end
		return construct("instrument", site(), opts, fill)
	end,
})

-- `keyboard`, `tuning`, and `filter` apply when the instrument is the
-- patch's root, and are rejected on an instrument inside a sequence.
---@class synth.Instrument
---@field waveform? synth.Periodic one cycle, fixed; set this or `wavetable`
---@field wavetable? synth.Wavetable frames blended by position; set this or `waveform`
---@field envelope? synth.Envelope
---@field voice_count? integer notes it can sound at once; 1 is mono
---@field keyboard? synth.Keyboard
---@field tuning? synth.Tuning
---@field filter? synth.Filter

defaults = {
	waveform = wave.periodic(wave.triangle),
	-- copied for each instrument, so none shares another's envelope
	envelope = envelope(),
	-- notes this instrument can sound at once; 1 is mono
	voice_count = 4,
}

return instrument
