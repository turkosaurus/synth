-- expect: envelope: expected an envelope, got a keyboard made at {file}:4
-- expect-variant: Error::WrongKind { what: "envelope", expected: "envelope", got: "keyboard", .. }
local synth = require("synth")
local keys = synth.input({})
return synth.instrument({ envelope = keys })
