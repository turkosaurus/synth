-- expect: instrument `a`: envelope made at {file}:4: `sustain` is 2; must be 0 to 1
-- expect-variant: Error::OutOfRange { field: "sustain", value: 2.0, .. }
local synth = require("synth")
local swell = synth.envelope({ sustain = 2 })
return synth.sequence({
	instruments = { a = synth.instrument({ envelope = swell }) },
	instrument = "a",
	steps = { { note = 60 } },
})
