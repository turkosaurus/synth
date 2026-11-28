-- expect: keyboard.quit and keyboard.quit_now must name at least one key between them
local synth = require("synth")
return synth.instrument({ keyboard = synth.keyboard({ quit = {}, quit_now = {} }) })
