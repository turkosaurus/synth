local synth = require("synth")
assert(not require("synth.wave").stale, "a copy matching its base loaded")
return synth.instrument({})
