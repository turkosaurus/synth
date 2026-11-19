-- requires a sibling module, one in a kind folder, and a folder with init.lua
local synth = require("synth")
local voice = require("voice")
local bass = require("instrument.bass")
local kit = require("kit")

assert(voice.voice_count == 3, "voice.lua")
assert(bass.voice_count == 2, "instrument/bass.lua")
assert(kit == "kit", "kit/init.lua")

return synth.instrument({ voice_count = voice.voice_count })
