-- Example song file for showcasing composition
-- and validating build output aurally.
local synth = require("synth")

return synth.sequence({
	bpm = 140,
	division = 4, -- subdivisions per beat; 4 = 16th notes, 8 = 32nd notes, etc.
	instrument = "tri", -- default for every pattern; stickiness resets per pattern
	filter = synth.filter.lowpass({ cutoff = 2000, resonance = 0.3 }), -- on the summed output

	-- non-default keyboard overrides
	input = {
		keyboard = synth.input.keyboard({
			-- qwerty, qwertz, azerty, dvorak, colemak, or your own
			layout = synth.input.keyboard.qwerty,
			octave_up = { "pageup" }, -- shift up one octave (unset: layout's)
			octave_down = { "pagedown" }, -- shift down one octave (unset: layout's)
			octave = 0, -- starting octave offset (0: layout default)
			octave_low = 3, -- octaves down from start selectable by octave_down
			octave_high = 2, -- octaves up from the start selectable by octave_up
			help = { "?" }, -- show the help screen (unset: layout's)
			stop = { "escape", "backspace" }, -- silence everything (non-empty)
			gate = 300, -- ms; note length when the terminal cannot report key release
			autorepeat_threshold = 60, -- ms; a repeat press this soon is OS autorepeat
			doubletap_threshold = 400, -- ms; a second press this soon is a double tap
			velocity = 90, -- 1 to 127, for every note
		}),
	},

	-- define the synthesizer
	instruments = {
		tri = synth.instrument({
			oscillator = synth.osc({ waveforms = { synth.wave.triangle } }),
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
		-- "chop" is an arbitrary variable name with musical significance,
		-- allowing phrases to be composed by naming them in a chain.
		chop = synth.sequence.list(function(add)
			for _ = 1, 2 do
				add({ note = 60, duration = 0.4 }, {})
				add({ note = 60, duration = 0.4 }, {})
			end
		end),

		-- You know the one. A finishing cliché.
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

	-- another arbitrary variable name holding a composition of phrases.
	chain = synth.sequence.list(function(add)
		for _ = 1, 1 do
			add("chop")
		end
		add("lick")
	end),
})
