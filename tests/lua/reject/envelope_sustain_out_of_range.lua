-- expect: envelope made at {file}:5: `sustain` is 1.5; must be 0 to 1
-- expect-variant: Error::OutOfRange { field: "sustain", .. }
local synth = require("synth")
return synth.instrument({
	envelope = synth.envelope({ sustain = 1.5 }),
})
