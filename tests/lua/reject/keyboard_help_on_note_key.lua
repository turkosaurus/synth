-- q plays a note on qwerty
local synth = require("synth")
return synth.instrument({ keyboard = synth.keyboard({ help = { "q" } }) })
