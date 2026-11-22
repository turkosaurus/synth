-- expect: `shape` must be a function, got 3
local synth = require("synth")
return synth.instrument({ waveform = synth.wave.periodic(3) })
