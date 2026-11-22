-- A waveform is a shape: phase (0.0 to 1.0) to amplitude (-1.0 to 1.0).
---@alias synth.Waveform fun(phase: number): number

local construct = require("synth.construct")

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

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

-- One waveform as an instrument's cycle, fixed for as long as a note sounds.
---@class synth.Periodic
---@field shape synth.Waveform

---@param shape synth.Waveform
---@return synth.Periodic
function wave.periodic(shape)
	return construct("wave.periodic", site(), { shape = shape }, {})
end

-- Waveforms baked one frame each and blended by `position`: n waves make
-- n - 1 transitions, spaced evenly over 0 to 1.
---@class synth.Wavetable
---@field waves synth.Waveform[] one frame each, in order
---@field slopes? synth.Curve[] transition i uses slopes[(i - 1) % #slopes + 1]; none is linear
---@field position? number 0 to 1: where between the first and last wave a note sounds

local table_defaults = {
	slopes = {},
	position = 0,
}

---@param opts synth.Wavetable
---@return synth.Wavetable
function wave.table(opts)
	return construct("wave.table", site(), opts, table_defaults)
end

return wave
