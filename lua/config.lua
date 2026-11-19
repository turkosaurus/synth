-- Optional settings for this machine, evaluated when program starts.
return {
	buffer_frames = 0, -- audio callback frames (0: defer to system default)
	queue = 256, -- events buffered between input and audio
	patches = "", -- folder of your own patches ("": ~/.local/share/synth/patches)
	tui = {
		poll = 16, -- ms between redraws and key reads
		meter_hold = 560, -- ms a peak holds before it falls
		meter_fall = 280, -- ms to fall across the whole meter
		meter_floor = -60, -- dB, the quietest level the meter shows
	},
}
