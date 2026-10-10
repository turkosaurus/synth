#!/usr/bin/env luajit

local slope = {}
local envelope = {}
local waveform = {}
local tuning = {
	tones = 12, -- notes per octave
	A4 = 440, -- Hz
	MIDIA4 = 69, -- note number
	-- TODO: temperament
}

-- linear slope for signed values
function slope.linear(initial, final, steps)
	if steps <= 0 then
		return {}
	end
	local step = (final - initial) / steps
	local result = {}
	for i = 1, steps do
		result[i] = initial + step * (i - 1)
	end
	return result
end

-- a plain gated note: fast edges so neither end clicks, and no decay
-- because sustain is already at the peak
local envelopeDefaults = {
	attackMS = 15,
	attackFunc = slope.linear,
	decayMS = 0,
	decayFunc = slope.linear,
	sustainLevel = 1, -- 0.0 to 1.0
	releaseMS = 15,
	releaseFunc = slope.linear,
}

-- Fills gaps without discarding anything. Building a fresh table here would
-- swallow a misspelled field silently; passing opts through lets it reach
-- Rust, which rejects it by name.
function envelope.new(opts)
	opts = opts or {}
	for key, default in pairs(envelopeDefaults) do
		if opts[key] == nil then
			opts[key] = default
		end
	end
	return opts
end

-- phase (0.0 to 1.0) to triangle wave (-1.0 to 1.0)
-- where phase=0.0 is amplitude=0.0
function waveform.triangle(phase)
	if phase < 0.25 then
		return 4 * phase
	elseif phase < 0.75 then
		return 2 - 4 * phase
	end
	return 4 * phase - 4
end

-- midi note number to Hz
function tuning.frequency(note)
	return tuning.A4 * 2 ^ ((note - tuning.MIDIA4) / tuning.tones)
end

return {
	slope = slope,
	envelope = envelope,
	waveform = waveform,
	tuning = tuning,
}
