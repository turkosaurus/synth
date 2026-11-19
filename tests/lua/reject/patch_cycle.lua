local synth = require("synth")
local loop = {}
loop.self = loop
return synth.instrument({ extra = loop })
