-- expect: {file}:3: attempt to index global 'jit' (a nil value)
---@diagnostic disable-next-line: undefined-global
return { jit.off() }
