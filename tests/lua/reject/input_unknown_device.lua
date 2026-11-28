-- expect: input: unknown field `midi`; expected one of keyboard
local synth = require("synth")
return synth.instrument({ input = { midi = {} } })
