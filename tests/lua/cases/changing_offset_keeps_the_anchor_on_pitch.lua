-- a mandolin layout: rows a 5th apart instead of a 4th
local instrument = require("synth.instrument")
local wave = require("synth.wave")
local envelope = require("synth.envelope")
local keyboard = require("synth.keyboard")
return instrument({
	oscillator = require("synth.osc")({ waveforms = { wave.triangle } }),
	envelope = envelope({}),
	keyboard = keyboard({ layout = keyboard.grid({ offset = 7 }) }),
})
