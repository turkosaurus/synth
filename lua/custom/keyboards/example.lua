-- A keyboard of your own. A patch uses it with
--   keyboard = require("keyboards.example")
local synth = require("synth")

return synth.keyboard({
	-- qwerty, qwertz, azerty, dvorak, colemak, or your own { name, notes, ... }
	layout = synth.keyboard.qwerty,
	-- octave = 0, -- start shift
	-- octave_low = 2, -- octaves down from the start
	-- octave_high = 3, -- octaves up from the start
	-- octave_up = { "pageup" },
	-- octave_down = { "pagedown" },
})
