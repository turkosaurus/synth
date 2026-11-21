-- slope defines linear and exponential slopes,
-- composed into waves, envelopes, and other time-varying values.
--
-- A curve is a shape;
-- a function from t (0..1; x-axis, time through an envelope stage)
-- to progression through the envelope's amplitude (also 0..1; y-axis, level).
-- Rust samples and scales a curve to each stage, so one curve serves
-- a rising attack and a falling release alike.
-- An example:
--   attack_curve = function(t) return t * t end
---@alias synth.Curve fun(t: number): number

local slope = {}

-- 0 is linear;
-- positive moves faster at the start,
-- negative faster at the end.
---@param curve number
---@return synth.Curve
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
---@return synth.Curve
function slope.linear()
	return slope.exponential(0)
end

return slope
