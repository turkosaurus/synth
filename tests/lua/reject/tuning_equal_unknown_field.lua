-- expect: :4: tuning.equal: unknown field 'A4'
-- A4 is the old name for reference_hz
local synth = require("synth")
return synth.instrument({ tuning = synth.tuning.equal({ A4 = 440 }) })
