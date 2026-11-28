local synth = require("synth")
local first = synth.input.keyboard({})
first.layout.notes.a = 0
first.stop[1] = "q"
assert(synth.input.keyboard({}).layout.notes.a ~= 0, "layout shared")
assert(synth.input.keyboard({}).stop[1] == "escape", "stop shared")
assert(synth.input.keyboard.qwerty.notes.a ~= 0, "template edited")

local bass = synth.instrument({})
bass.envelope.attack = 999
assert(synth.instrument({}).envelope.attack ~= 999, "envelope shared")
assert(getmetatable(bass.envelope).kind == "envelope", "stamp lost")
return synth.instrument({})
