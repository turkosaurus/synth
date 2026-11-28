-- expect: instrument `a`: envelope.decay_curve returned NaN at t = 0; must be a number from 0 to 1
-- expect-variant: Error::CurveNotFinite { field: "decay_curve", t: 0.0, .. }
local synth = require("synth")
local nan = synth.envelope({
	decay_curve = function(t)
		return t / t
	end,
})
return synth.sequence({
	instruments = { a = synth.instrument({ envelope = nan }) },
	instrument = "a",
	steps = { { note = 60 } },
})
