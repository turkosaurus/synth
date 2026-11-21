local wave = require("synth.wave")
local envelope = require("synth.envelope")

local new -- the constructor, defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.instrument
---@overload fun(opts?: synth.Instrument): synth.Instrument
local instrument = setmetatable({}, {
	__call = function(_, opts)
		return setmetatable(new(opts), { kind = "instrument", at = site() })
	end,
})

-- `keyboard`, `tuning`, and `filter` apply when the instrument is the
-- patch's root.
---@class synth.Instrument
---@field waveform? synth.Waveform
---@field envelope? synth.Envelope
---@field voice_count? integer notes it can sound at once; 1 is mono
---@field keyboard? synth.Keyboard
---@field tuning? synth.Tuning
---@field filter? synth.Filter

-- Copies the caller's fields, then fills in defaults.
-- Unknown fields are propagated for error reporting.
new = function(opts)
	local out = {}
	for key, value in pairs(opts or {}) do
		out[key] = value
	end
	if out.waveform == nil then
		out.waveform = wave.triangle
	end
	-- a fresh envelope per instrument, never one shared between them
	if out.envelope == nil then
		out.envelope = envelope()
	end
	-- notes this instrument can sound at once; 1 is mono
	if out.voice_count == nil then
		out.voice_count = 4
	end
	return out
end

return instrument
