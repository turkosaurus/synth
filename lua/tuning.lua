#!/usr/bin/env luajit

local tuning = {
	tones = 12, -- notes per octave
	A4 = 440, -- Hz
	MIDIA4 = 69, -- note number
}

-- midi note number to Hz
function tuning.frequency(note)
	return tuning.A4 * 2 ^ ((note - tuning.MIDIA4) / tuning.tones)
end

return tuning
