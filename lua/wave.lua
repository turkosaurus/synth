#!/usr/bin/env luajit

local wave = {}

-- phase (0.0 to 1.0) to triangle wave (-1.0 to 1.0)
-- where phase=0.0 is amplitude=0.0
function wave.triangle(phase)
	if phase < 0.25 then
		return 4 * phase
	elseif phase < 0.75 then
		return 2 - 4 * phase
	end
	return 4 * phase - 4
end

return wave
