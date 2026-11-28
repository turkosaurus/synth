-- expect: envelope.attack_curve returned a function, not a number: a curve constructor needs its (), as in synth.slope.exponential(2)
-- expect-variant: Error::CurveUncalled
-- the mistake is the missing (): the constructor is sampled as if it were the curve
local synth = require("synth")
return synth.instrument({
	envelope = synth.envelope({ attack = 10, attack_curve = synth.slope.exponential }),
})
