#!/usr/bin/env luajit

local sequence = require("synth.sequence")
local instrument = require("synth.instrument")
local wave = require("synth.wave")
local envelope = require("synth.envelope")

return sequence.new({
	bpm = 140,
	division = 4, -- subdivisions per beat; 4 = 16th notes, 8 = 32nd notes, etc.
	instrument = "tri", -- default for every pattern; stickiness resets per pattern

	instruments = {
		tri = instrument.new({
			waveform = wave.triangle,
			envelope = envelope.new({
				attackMS = 100,
				decayMS = 150,
				sustainLevel = 0.7,
				releaseMS = 50,
			}),
		}),
	},

	-- one step is one grid slot; duration is slots to sound, so >1 holds over
	patterns = {
		chop = sequence.list(function(add)
			for _ = 1, 2 do
				add({ note = 60, duration = 0.4 }, {})
				add({ note = 60, duration = 0.4 }, {})
			end
		end),

		lick = {
			{ note = 60, duration = 0.1 },
			{ note = 62, duration = 0.2 },
			{ note = 63 },
			{ note = 65 },
			{ note = 62, duration = 0.9 },
			{},
			{ note = 58 },
			{ note = 60, duration = 1.0 },
		},
	},

	chain = sequence.list(function(add)
		for _ = 1, 1 do
			add("chop")
		end
		add("lick")
	end),
})
