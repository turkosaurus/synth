-- expect: {file}:2: attempt to call global 'loadfile' (a nil value)
return { loadfile("/etc/passwd") }
