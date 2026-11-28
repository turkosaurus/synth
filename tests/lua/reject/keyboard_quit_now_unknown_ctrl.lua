-- expect: keyboard.quit_now: unknown key 'ctrl-tab'
local synth = require("synth")
return synth.instrument({ keyboard = synth.input({ quit_now = { "ctrl-tab" } }) })
