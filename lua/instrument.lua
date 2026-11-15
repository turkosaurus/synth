local wave = require("synth.wave")
local envelope = require("synth.envelope")

local new -- the constructor, defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.instrument
---@overload fun(opts?: table): table
local instrument = setmetatable({}, {
	__call = function(_, opts)
		return setmetatable(new(opts), { kind = "instrument", at = site() })
	end,
})

-- A new table, built the way envelope builds one: the caller's fields, then
-- defaults, unknown fields kept for Rust to name.
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
		out.voice_count = 1
	end
	return out
end

return instrument
