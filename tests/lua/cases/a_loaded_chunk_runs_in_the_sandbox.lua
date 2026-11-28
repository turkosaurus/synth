local parts = { "return ", "1 + 1" }
local i = 0
local function pieces()
	i = i + 1
	return parts[i]
end
---@diagnostic disable-next-line: deprecated
return load("return io")(), loadstring("return os")(), load(pieces)()
