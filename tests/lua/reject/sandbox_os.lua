-- expect: {file}:2: attempt to index global 'os' (a nil value)
return { os.execute("id") }
