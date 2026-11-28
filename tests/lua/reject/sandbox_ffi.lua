-- expect: {file}:3: attempt to index global 'ffi' (a nil value)
---@diagnostic disable-next-line: undefined-global
return { ffi.C }
