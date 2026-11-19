-- the keyboard used to take grid fields directly; they belong to the grid
local synth = require("synth")
return synth.instrument({ keyboard = synth.keyboard({ offset = 7 }) })
