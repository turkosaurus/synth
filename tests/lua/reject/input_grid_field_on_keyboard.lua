-- expect: keyboard made at {file}:4: unknown field `offset`
-- the keyboard used to take grid fields directly; they belong to the grid
local synth = require("synth")
return synth.instrument({ keyboard = synth.input({ offset = 7 }) })
