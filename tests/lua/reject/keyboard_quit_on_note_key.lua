-- expect: keyboard.layout plays note 74 on 'q', which keyboard.quit also binds
-- q plays a note on qwerty
local synth = require("synth")
return synth.instrument({ keyboard = synth.input({ quit = { "q" } }) })
