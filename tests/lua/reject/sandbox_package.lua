-- expect: {file}:2: attempt to index global 'package' (a nil value)
return { package.loadlib("libc.so.6", "system") }
