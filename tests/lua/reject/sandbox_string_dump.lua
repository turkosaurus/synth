-- expect: {file}:2: attempt to call field 'dump' (a nil value)
return { string.dump(print) }
