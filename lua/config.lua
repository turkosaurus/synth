-- Settings for this machine, read by synth at startup. Song files hold the
-- music; this holds what depends on the machine. `synth doctor` shows the
-- settings in effect. Every setting is optional.
return {
	-- frames per audio callback; smaller is lower latency, larger is
	-- steadier. 0 lets the platform decide.
	buffer_frames = 0,
}
