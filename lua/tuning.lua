-- A tuning is a function from MIDI note number to Hz. Rust bakes it into a
-- table for every note, so any function works: equal temperament below, or a
-- patch's own just intonation, cents table, or non-octave scale.
---@alias synth.Tuning fun(note: integer): number

local tuning = {}

---@class (exact) synth.TuningEqual
---@field reference_hz? number Hz of the reference note
---@field reference_note? integer MIDI note number; 69 is A4
---@field tones? number notes per octave
local defaults = {
	reference_hz = 440, -- Hz of the reference note
	reference_note = 69, -- MIDI note number; 69 is A4
	tones = 12, -- notes per octave
}

-- Equal temperament: `tones` equal steps per octave, pinned so
-- `reference_note` sounds at `reference_hz`.
---@param opts? synth.TuningEqual
---@return synth.Tuning
function tuning.equal(opts)
	opts = opts or {}
	for key in pairs(opts) do
		if defaults[key] == nil then
			error("tuning.equal: unknown field '" .. tostring(key) .. "'", 2)
		end
	end
	local hz = opts.reference_hz or defaults.reference_hz
	local reference = opts.reference_note or defaults.reference_note
	local tones = opts.tones or defaults.tones
	return function(note)
		return hz * 2 ^ ((note - reference) / tones)
	end
end

return tuning
