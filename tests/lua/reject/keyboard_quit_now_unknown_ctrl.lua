-- expect: keyboard.quit_now: unknown key 'ctrl-tab'
local synth = require("synth")
return synth.instrument({ input = { keyboard = synth.input.keyboard({ quit_now = { "ctrl-tab" } }) } })
