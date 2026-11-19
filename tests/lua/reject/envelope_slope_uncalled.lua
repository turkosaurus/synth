-- expect: slope.exponential(amount) returns a slope; it is not one itself
-- the mistake is the missing (), so Rust would call the constructor itself
local synth = require("synth")
return synth.instrument({
	envelope = synth.envelope({ attack = 10, attack_curve = synth.slope.exponential }),
})
