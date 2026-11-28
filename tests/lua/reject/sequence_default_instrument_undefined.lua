-- expect: step 1 names instrument `b`, which is not defined
-- expect-variant: Error::UndefinedInstrument
local synth = require("synth")
return synth.sequence({
	instruments = { a = synth.instrument({}) },
	instrument = "b",
	steps = { { note = 60 } },
})
