-- synth/filter.lua sits beside this file and loads in place of the built-in
local synth = require("synth")

assert(require("synth.filter").shadowed, "the copy beside the patch")
assert(synth.filter.shadowed, "the root re-exports the same copy")

return synth.instrument({})
