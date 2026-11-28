-- expect: step 2: `note` is 200; must be 0 to 127
-- expect-variant: Error::OutOfRange { what: "step 2", field: "note", value: 200.0, .. }
local synth = require("synth")
return synth.sequence({
	instruments = { a = synth.instrument({}) },
	instrument = "a",
	steps = { { note = 60 }, { note = 200 } },
})
