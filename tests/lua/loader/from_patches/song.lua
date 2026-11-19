-- instrument.lead is only in the patches folder; voice is in both
local synth = require("synth")
local lead = require("instrument.lead")
local voice = require("voice")

assert(lead.voice_count == 5, "instrument/lead.lua from the patches folder")
assert(voice.from == "song", "the song's own voice.lua comes first")

return synth.instrument({ voice_count = lead.voice_count })
