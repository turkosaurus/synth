-- A waveform is a shape: phase (0.0 to 1.0) to amplitude (-1.0 to 1.0).
---@alias synth.Waveform fun(phase: number): number

local wave = {}

-- phase (0.0 to 1.0) to triangle wave (-1.0 to 1.0)
-- where phase=0.0 is amplitude=0.0
---@param phase number
---@return number
function wave.triangle(phase)
	if phase < 0.25 then
		return 4 * phase
	elseif phase < 0.75 then
		return 2 - 4 * phase
	end
	return 4 * phase - 4
end

-- phase (0.0 to 1.0) to sine wave (-1.0 to 1.0)
---@param phase number
---@return number
function wave.sine(phase)
	return math.sin(2 * math.pi * phase)
end

-- phase (0.0 to 1.0) to saw wave (-1.0 to 1.0)
-- rises from 0, so the jump falls mid-cycle and the wrap is smooth
---@param phase number
---@return number
function wave.saw(phase)
	if phase < 0.5 then
		return 2 * phase
	end
	return 2 * phase - 2
end

-- phase (0.0 to 1.0) to square wave (-1.0 to 1.0)
---@param phase number
---@return number
function wave.square(phase)
	if phase < 0.5 then
		return 1
	end
	return -1
end

return wave
