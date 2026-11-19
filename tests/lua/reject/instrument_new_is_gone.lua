-- expect: :4: attempt to call field 'new' (a nil value)
-- constructors are called directly; the old `.new` entry points are gone
local synth = require("synth")
return synth.instrument.new({})
