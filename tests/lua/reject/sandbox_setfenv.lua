-- expect: {file}:4: attempt to call global 'setfenv' (a nil value)
-- either one reaches the full globals from a sandboxed function
---@diagnostic disable-next-line: deprecated
return { setfenv(print, {}) }
