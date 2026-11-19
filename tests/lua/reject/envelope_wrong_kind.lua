local synth = require("synth")
local keys = synth.keyboard({})
return synth.instrument({ envelope = keys })
