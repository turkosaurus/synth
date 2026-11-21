-- A chord of `voices` notes, one per step, each held far longer than any
-- bench runs, so after the last step every voice sustains and each frame
-- costs the same.
local synth = require("synth")

return function(voices)
	local steps = {}
	for i = 1, voices do
		steps[i] = { note = 36 + i, duration = 1e9 }
	end
	return synth.sequence({
		bpm = 600,
		division = 8,
		instrument = "tri",
		filter = synth.filter.lowpass({ cutoff = 2000, resonance = 0.3 }),
		instruments = {
			tri = synth.instrument({
				waveform = synth.wave.triangle,
				voice_count = voices,
			}),
		},
		patterns = { chord = steps },
		chain = { "chord" },
	})
end
