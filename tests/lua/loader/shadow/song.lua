-- synth/wave.lua sits beside this file, but the built-in must load
local synth = require("synth")
local wave = require("synth.wave")

assert(wave.triangle, "the built-in wave")

return synth.instrument({})
