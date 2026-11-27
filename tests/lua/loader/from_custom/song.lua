-- instruments.lead is only in the custom folder; voice is in both
local synth = require("synth")
local lead = require("instruments.lead")
local voice = require("voice")

assert(lead.voice_count == 5, "instruments/lead.lua from the custom folder")
assert(voice.from == "song", "the song's own voice.lua comes first")

return synth.instrument({ voice_count = lead.voice_count })
