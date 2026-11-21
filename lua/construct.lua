-- The one way every constructor builds its value: the caller's fields, then
-- a deep copy of each default left out, stamped with its kind and where it
-- was made. Not part of `require("synth")`; the constructors use it.

-- Tables are copied all the way down, so no two values share a default and
-- editing one changes no other. A copied table keeps its metatable, so a
-- default that is itself a constructed value keeps its stamp.
local function copy(value)
	if type(value) ~= "table" then
		return value
	end
	local out = {}
	for key, inner in pairs(value) do
		out[key] = copy(inner)
	end
	return setmetatable(out, getmetatable(value))
end

-- `at` comes from `synth.site`, called in the public constructor itself,
-- since it names the line two calls up. The caller's table is left alone.
-- Unknown fields are kept, so a misspelling reaches Rust and gets named
-- instead of silently taking a default.
---@param kind string
---@param at string
---@param opts? table
---@param defaults table
---@return table
local function construct(kind, at, opts, defaults)
	local out = {}
	for key, value in pairs(opts or {}) do
		out[key] = value
	end
	for key, default in pairs(defaults) do
		if out[key] == nil then
			out[key] = copy(default)
		end
	end
	return setmetatable(out, { kind = kind, at = at })
end

return construct
