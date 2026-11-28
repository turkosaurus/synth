-- a patch reusing one options table must not find defaults written into it
local synth = require("synth")
local opts = { attack = 5 }
local first = synth.envelope(opts)
assert(opts.release == nil, "defaults leaked into the caller's table")
assert(first ~= opts, "returned the caller's table")
return synth.instrument({ envelope = first })
