-- expect: {file}:2: attempt to index global 'debug' (a nil value)
return { debug.getregistry() }
