-- expect: keyboard.layout plays note 74 on 'q', which keyboard.quit also binds
-- q plays a note on qwerty
local synth = require("synth")
return synth.instrument({ input = { keyboard = synth.input.keyboard({ quit = { "q" } }) } })
