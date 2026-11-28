-- expect: {file}:2: attempt to index global 'io' (a nil value)
return { io.popen("id") }
