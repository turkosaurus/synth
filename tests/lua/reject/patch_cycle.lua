-- expect: extra.self contains itself; a patch cannot hold a cycle
local synth = require("synth")
local loop = {}
loop.self = loop
return synth.instrument({ extra = loop })
