-- expect: `waveform` must be a synth.wave.periodic(…), got a function
local synth = require("synth")
return synth.instrument({ waveform = synth.wave.saw })
