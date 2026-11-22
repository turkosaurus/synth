-- An instrument of your own. A patch uses it with
--   instruments = { lead = require("instrument.example") }
-- Copy this file to start another; the name is what require takes.
local synth = require("synth")

-- a waveform is any function from phase (0 to 1) to a sample (-1 to 1);
-- one that ends where it starts wraps without a click
local function sine(phase)
	return math.sin(2 * math.pi * phase)
end

return synth.instrument({
	oscillator = synth.osc.periodic({ wave = sine }), -- or synth.wave.triangle
	envelope = synth.envelope({
		attack = 5, -- ms
		release = 200, -- ms
		-- decay = 100, -- ms
		-- sustain = 0.7, -- 0 to 1
		-- attack_curve = synth.slope.exponential(2),
	}),
	-- voice_count = 4, -- notes at once; 1 is mono
})
