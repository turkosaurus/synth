-- expect: tuning must return Hz for note 0:
-- expect-variant: Error::TuningCall { note: 0, .. }
local synth = require("synth")
return synth.instrument({
	tuning = function()
		return "a"
	end,
})
