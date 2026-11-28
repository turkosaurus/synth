-- expect: step 1 names instrument `b`, which is not defined
-- expect-variant: Error::UndefinedInstrument { step: 1, name: "b" }
local synth = require("synth")
return synth.sequence({
	instruments = { a = synth.instrument({}) },
	instrument = "b",
	steps = { { note = 60 } },
})
