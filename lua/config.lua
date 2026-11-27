-- Optional settings for this machine, evaluated when program starts.

---@class synth.ConfigBudget
---@field warn? number where a reading turns yellow
---@field danger? number where it turns red

---@class synth.ConfigLatency
---@field window? number ms of readings each row keeps
---@field records? integer timing records queued for the view, allocated once at start
---@field floor? number ms, left end of the scale
---@field ceil? number ms, right end of the scale
---@field input? synth.ConfigBudget ms, key read until pushed
---@field render? synth.ConfigBudget ms, pushed until the callback applied it
---@field output? synth.ConfigBudget ms, the driver's playback - callback
---@field total? synth.ConfigBudget ms, input + render + output per key

---@class synth.ConfigTui
---@field poll? number ms between redraws and key reads
---@field meter_hold? number ms a peak holds before it falls
---@field meter_fall? number ms to fall across the whole meter
---@field meter_floor? number dB, the quietest level the meter shows
---@field meter_latency? synth.ConfigLatency
---@field meter_fps? synth.ConfigBudget frames per second; the view warns below these

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
		meter_latency = {
			window = 5000, -- ms of readings each row keeps
			records = 1024, -- timing records queued for the view, allocated once at start
			floor = 0.01, -- ms, left end of the scale
			ceil = 100, -- ms, right end of the scale
			input = { warn = 1, danger = 5 },
			render = { warn = 5, danger = 10 },
			output = { warn = 10, danger = 20 },
			total = { warn = 10, danger = 20 },
		},
		meter_fps = { warn = 30, danger = 24 },
	},
}

return config
