-- expect: extra.self contains itself; a patch cannot hold a cycle
-- expect-variant: Error::Cycle { what: "extra.self" }
local synth = require("synth")
local loop = {}
loop.self = loop
return synth.instrument({ extra = loop })
