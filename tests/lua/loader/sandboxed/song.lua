local probe = require("probe")
assert(probe.io == nil and probe.os == nil, "a module reached io or os")
return require("synth").instrument({})
