-- expect: {file}:2: attempt to call global 'dofile' (a nil value)
return { dofile("/etc/passwd") }
