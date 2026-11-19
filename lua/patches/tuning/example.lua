-- A tuning of your own: any function from MIDI note to Hz. A patch uses it
-- with
--   tuning = require("tuning.example")
local synth = require("synth")

return synth.tuning.equal({
	reference_hz = 440, -- Hz of the reference note
	-- reference_note = 69, -- MIDI; 69 is A4
	-- tones = 12, -- notes per octave
})
