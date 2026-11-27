-- Optional settings for this machine, evaluated when program starts.

---@class synth.ConfigTui
---@field poll? number ms between redraws and key reads
---@field meter_hold? number ms a peak holds before it falls
---@field meter_fall? number ms to fall across the whole meter
---@field meter_floor? number dB, the quietest level the meter shows

---@class synth.Config
---@field buffer_frames? integer audio callback frames (0: system default)
---@field queue? integer buffer size: input -> audio
---@field custom_dir? string custom folder
---@field tui? synth.ConfigTui
local config = {
	buffer_frames = 0, -- audio callback frames (0: system default)
	queue = 256, -- buffer size: input -> audio
	-- custom_dir = [[{default}]] -- custom folder
	tui = {
		poll = 16, -- ms between redraws and key reads
		meter_hold = 560, -- ms a peak holds before it falls
		meter_fall = 280, -- ms to fall across the whole meter
		meter_floor = -60, -- dB, the quietest level the meter shows
	},
}

return config
