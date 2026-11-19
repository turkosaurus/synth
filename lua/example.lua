local synth = require("synth")

return synth.sequence({
	bpm = 140,
	division = 4, -- subdivisions per beat; 4 = 16th notes, 8 = 32nd notes, etc.
	instrument = "tri", -- default for every pattern; stickiness resets per pattern
	filter = synth.filter.lowpass({ cutoff = 2000, resonance = 0.3 }), -- on the summed output
	-- every keyboard field, set away from its default (see keyboard.lua)
	keyboard = synth.keyboard({
		-- qwerty, qwertz, azerty, dvorak, colemak, or your own
		layout = synth.keyboard.qwerty,
		-- replace the layout's octave keys; a key name or a single character
		octave_up = { "pageup" },
		octave_down = { "pagedown" },
		octave = 0, -- start shift; 0 plays the layout as written
		octave_low = 3, -- octaves down from the start
		octave_high = 2, -- octaves up from the start
		help = { "?" },
		stop = { "escape", "backspace" }, -- silence everything; must not be empty
		gate = 300, -- ms; note length when the terminal cannot report key release
		repeat_window = 600, -- ms; a repeat press this soon is OS autorepeat
		velocity = 90, -- 1 to 127, for every note
	}),

	instruments = {
		tri = synth.instrument({
			waveform = synth.wave.triangle,
			envelope = synth.envelope({
				attack = 100, -- ms
				decay = 150,
				sustain = 0.7,
				release = 50,
			}),
		}),
	},

	-- one step is one grid slot; duration is slots to sound, so >1 holds over
	patterns = {
		chop = synth.sequence.list(function(add)
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

	chain = synth.sequence.list(function(add)
		for _ = 1, 1 do
			add("chop")
		end
		add("lick")
	end),
})
