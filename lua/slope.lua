#!/usr/bin/env luajit

local slope = {}

-- 0 is linear, higher is steeper, negative bends the other way
function slope.exponential(curve, extra)
	assert(extra == nil, "slope.exponential(amount) returns a slope; it is not one itself")

	return function(initial, final, steps)
		if steps <= 0 then
			return {}
		end
		local result = {}

		-- linear (avoids division by zero and exp(0) == 1)
		if curve == 0 then
			for i = 1, steps do
				result[i] = initial + (final - initial) * (i - 1) / steps
			end
			return result
		end

		local scale = 1 - math.exp(-curve)
		for i = 1, steps do
			local t = (i - 1) / steps
			result[i] = initial + (final - initial) * (1 - math.exp(-curve * t)) / scale
		end
		return result
	end
end

-- linear slope for signed values
function slope.linear(extra)
	assert(extra == nil, "slope.linear() returns a slope; it is not one itself")
	return slope.exponential(0)
end

return slope
