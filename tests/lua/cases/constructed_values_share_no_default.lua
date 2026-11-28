local synth = require("synth")
local first = synth.input({})
first.layout.notes.a = 0
first.stop[1] = "q"
assert(synth.input({}).layout.notes.a ~= 0, "layout shared")
assert(synth.input({}).stop[1] == "escape", "stop shared")
assert(synth.input.qwerty.notes.a ~= 0, "template edited")

local bass = synth.instrument({})
bass.envelope.attack = 999
assert(synth.instrument({}).envelope.attack ~= 999, "envelope shared")
assert(getmetatable(bass.envelope).kind == "envelope", "stamp lost")
return synth.instrument({})
