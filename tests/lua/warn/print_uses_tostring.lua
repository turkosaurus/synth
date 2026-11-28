-- expect: print 7 custom\tnil\ttrue
local custom = setmetatable({}, {
	__tostring = function()
		return "custom"
	end,
})
print(custom, nil, true)
return require("synth").instrument({})
