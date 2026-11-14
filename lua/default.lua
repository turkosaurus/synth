-- What synth plays when no file is given: the default instrument, with four
-- voices so notes can overlap.
local instrument = require("synth.instrument")

return instrument.new({ voiceCount = 4 })
