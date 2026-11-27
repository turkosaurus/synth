-- Optional settings for this machine, evaluated when program starts.

---@class synth.ConfigTui
---@field poll? number ms between redraws and key reads
---@field meter_hold? number ms a peak holds before it falls
---@field meter_fall? number ms to fall across the whole meter
---@field meter_floor? number dB, the quietest level the meter shows
---@field latency_window? number ms of readings each latency row keeps
---@field latency_floor? number ms at the left end of the latency scale
---@field latency_ceil? number ms at the right end of the latency scale
---@field latency_input_warn? number ms where input turns yellow
---@field latency_input_danger? number ms where input turns red
---@field latency_render_warn? number ms where render turns yellow
---@field latency_render_danger? number ms where render turns red
---@field latency_output_warn? number ms where output turns yellow
---@field latency_output_danger? number ms where output turns red
---@field latency_total_warn? number ms where total turns yellow
---@field latency_total_danger? number ms where total turns red
---@field fps_warn? integer frames per second below which the view turns yellow
---@field fps_danger? integer frames per second below which the view turns red

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
		latency_window = 5000, -- ms of readings each latency row keeps
		latency_floor = 0.01, -- ms at the left end of the latency scale
		latency_ceil = 100, -- ms at the right end of the latency scale
		latency_input_warn = 1, -- ms where input turns yellow
		latency_input_danger = 5, -- ms where input turns red
		latency_render_warn = 5, -- ms where render turns yellow
		latency_render_danger = 10, -- ms where render turns red
		latency_output_warn = 10, -- ms where output turns yellow
		latency_output_danger = 20, -- ms where output turns red
		latency_total_warn = 10, -- ms where total turns yellow
		latency_total_danger = 20, -- ms where total turns red
		fps_warn = 30, -- frames per second below which the view turns yellow
		fps_danger = 24, -- frames per second below which the view turns red
	},
}

return config
