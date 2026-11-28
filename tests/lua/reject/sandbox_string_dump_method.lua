-- expect: {file}:4: attempt to call method 'dump' (a nil value)
-- every string reaches the string library through its metatable
---@diagnostic disable-next-line: param-type-mismatch
return { (""):dump() }
