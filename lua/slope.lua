-- slope defines linear and exponential slopes,
-- composed into waves, envelopes, and other time-varying values.
--
-- A curve is a shape: a function from t (0..1) across one envelope stage (y-axis/amplitude),
-- to how far along that stage's change the envelope is (also 0..1; x-axis/time).
-- Rust samples and scales a curve to each stage, so one curve serves
-- a rising attack and a falling release alike.
-- An example:
--   attack_curve = function(t) return t * t end
local slope = {}

-- 0 is linear, higher moves faster at the start of the stage,
-- negative faster at the end
function slope.exponential(curve)
	-- exp(0) == 1 would divide by zero
	if curve == 0 then
		return function(t)
			return t
		end
	end
	local scale = 1 - math.exp(-curve)
	return function(t)
		return (1 - math.exp(-curve * t)) / scale
	end
end

-- the same rate of change across the whole stage
function slope.linear()
	return slope.exponential(0)
end

return slope
