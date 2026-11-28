-- expect: print 3 cutoff\t800
local synth = require("synth")
print("cutoff", 800)
return synth.instrument({})
