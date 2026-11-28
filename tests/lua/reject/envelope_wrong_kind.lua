-- expect: envelope: expected an envelope, got a keyboard made at {file}:4
-- expect-variant: Error::WrongKind
local synth = require("synth")
local keys = synth.keyboard({})
return synth.instrument({ envelope = keys })
