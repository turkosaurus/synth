-- expect: tuning returned -440 Hz for note 69; must be positive and finite
-- expect-variant: Error::TuningHz { note: 69, hz: -440.0 }
local synth = require("synth")
return synth.instrument({
	tuning = function(note)
		if note == 69 then
			return -440
		end
		return 440
	end,
})
