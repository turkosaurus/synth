-- expect: chain names pattern 'nope', which is not defined
local synth = require("synth")
return synth.sequence({
	patterns = { x = {} },
	chain = { "nope" },
})
