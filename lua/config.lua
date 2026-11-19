-- Optional settings for this machine, evaluated when program starts.
return {
	buffer_frames = 0, -- audio callback frames (0: system default)
	queue = 256, -- buffer size: input -> audio
	-- patch_dir = [[{default}]] -- custom patches folder
	tui = {
		poll = 16, -- ms between redraws and key reads
		meter_hold = 560, -- ms a peak holds before it falls
		meter_fall = 280, -- ms to fall across the whole meter
		meter_floor = -60, -- dB, the quietest level the meter shows
	},
}
