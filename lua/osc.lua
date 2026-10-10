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

function envelope.new(opts)
	opts = opts or {}
	return {
		attackMS = opts.attackMS or 0,
		attackFunc = opts.attackFunc or slope.linear,
		decayMS = opts.decayMS or 0,
		decayFunc = opts.decayFunc or slope.linear,
		sustainMS = opts.sustainMS or 200, -- ignored if gated
		sustainLevel = opts.sustainLevel or 1, -- 0.0 to 1.0
		releaseMS = opts.releaseMS or 0,
		releaseFunc = opts.releaseFunc or slope.linear,
	}
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
