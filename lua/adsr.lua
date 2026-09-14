#!/usr/bin/env luajit
--[[
  PortAudio ADSR Synthesizer (Blocking I/O version)
  Requires: LuaJIT and libportaudio installed

  macOS: brew install portaudio
  Linux: apt-get install libportaudio2 portaudio19-dev

  Usage: luajit adsr_synth_blocking.lua
]]

local ffi = require("ffi")

-- PortAudio FFI bindings
ffi.cdef([[
  typedef int PaError;
  typedef double PaTime;
  typedef unsigned long PaSampleFormat;
  typedef unsigned long PaStreamFlags;

  typedef struct PaStreamParameters {
    int device;
    int channelCount;
    PaSampleFormat sampleFormat;
    double suggestedLatency;
    void *hostApiSpecificStreamInfo;
  } PaStreamParameters;

  typedef void PaStream;

  PaError Pa_Initialize(void);
  PaError Pa_Terminate(void);
  const char* Pa_GetErrorText(PaError errorCode);
  int Pa_GetDefaultOutputDevice(void);

  PaError Pa_OpenStream(
    PaStream** stream,
    const PaStreamParameters *inputParameters,
    const PaStreamParameters *outputParameters,
    double sampleRate,
    unsigned long framesPerBuffer,
    PaStreamFlags streamFlags,
    void *streamCallback,
    void *userData
  );

  PaError Pa_StartStream(PaStream *stream);
  PaError Pa_StopStream(PaStream *stream);
  PaError Pa_CloseStream(PaStream *stream);
  PaError Pa_WriteStream(PaStream *stream, const void *buffer, unsigned long frames);
]])

-- Load PortAudio library
local pa
if ffi.os == "OSX" then
	pa = ffi.load("portaudio")
elseif ffi.os == "Linux" then
	pa = ffi.load("portaudio.so.2")
else
	pa = ffi.load("portaudio")
end

-- Constants
local paFloat32 = 0x00000001
local paNoFlag = 0
local paClipOff = 0x00000001
local SAMPLE_RATE = 44100
local FRAMES_PER_BUFFER = 512
local PI2 = math.pi * 2

-- ADSR Envelope Generator
local ADSR = {}
ADSR.__index = ADSR

function ADSR.new(attack, decay, sustain, release)
	local self = setmetatable({}, ADSR)
	self.attack_time = attack or 0.1 -- seconds
	self.decay_time = decay or 0.1 -- seconds
	self.sustain_level = sustain or 0.7 -- 0-1
	self.release_time = release or 0.3 -- seconds

	self.state = "idle" -- idle, attack, decay, sustain, release
	self.current_level = 0.0
	self.sample_rate = SAMPLE_RATE
	self.time = 0

	return self
end

function ADSR:note_on()
	self.state = "attack"
	self.time = 0
end

function ADSR:note_off()
	if self.state ~= "idle" then
		self.state = "release"
		self.time = 0
	end
end

function ADSR:process()
	if self.state == "idle" then
		self.current_level = 0.0
	elseif self.state == "attack" then
		self.time = self.time + 1
		local progress = self.time / (self.attack_time * self.sample_rate)
		self.current_level = progress

		if progress >= 1.0 then
			self.current_level = 1.0
			self.state = "decay"
			self.time = 0
		end
	elseif self.state == "decay" then
		self.time = self.time + 1
		local progress = self.time / (self.decay_time * self.sample_rate)
		self.current_level = 1.0 - (progress * (1.0 - self.sustain_level))

		if progress >= 1.0 then
			self.current_level = self.sustain_level
			self.state = "sustain"
			self.time = 0
		end
	elseif self.state == "sustain" then
		self.current_level = self.sustain_level
	elseif self.state == "release" then
		self.time = self.time + 1
		local start_level = self.current_level
		local progress = self.time / (self.release_time * self.sample_rate)
		self.current_level = start_level * (1.0 - progress)

		if progress >= 1.0 then
			self.current_level = 0.0
			self.state = "idle"
			self.time = 0
		end
	end

	return self.current_level
end

function ADSR:is_active()
	return self.state ~= "idle"
end

-- Simple Oscillator
local Oscillator = {}
Oscillator.__index = Oscillator

function Oscillator.new(frequency)
	local self = setmetatable({}, Oscillator)
	self.frequency = frequency or 440.0 -- A4
	self.phase = 0.0
	self.sample_rate = SAMPLE_RATE
	return self
end

function Oscillator:set_frequency(freq)
	self.frequency = freq
end

function Oscillator:generate_sine()
	local sample = math.sin(self.phase)
	self.phase = self.phase + (PI2 * self.frequency / self.sample_rate)

	-- Wrap phase to prevent floating point drift
	if self.phase >= PI2 then
		self.phase = self.phase - PI2
	end

	return sample
end

function Oscillator:generate_square()
	local sample = self.phase < math.pi and 1.0 or -1.0
	self.phase = self.phase + (PI2 * self.frequency / self.sample_rate)

	if self.phase >= PI2 then
		self.phase = self.phase - PI2
	end

	return sample
end

function Oscillator:generate_saw()
	local sample = (self.phase / math.pi) - 1.0
	self.phase = self.phase + (PI2 * self.frequency / self.sample_rate)

	if self.phase >= PI2 then
		self.phase = self.phase - PI2
	end

	return sample
end

-- Helper function to check PortAudio errors
local function pa_check(err)
	if err ~= 0 then
		error("PortAudio error: " .. ffi.string(pa.Pa_GetErrorText(err)))
	end
end

-- Generate audio buffer
local function generate_audio(oscillator, envelope, num_frames, master_volume)
	local buffer = ffi.new("float[?]", num_frames * 2) -- Stereo

	for i = 0, num_frames - 1 do
		local osc_sample = oscillator:generate_sine()
		local env_level = envelope:process()
		local sample = osc_sample * env_level * master_volume

		-- Stereo output
		buffer[i * 2] = sample -- Left
		buffer[i * 2 + 1] = sample -- Right
	end

	return buffer
end

-- Main program
local function main()
	print("PortAudio ADSR Synthesizer (Blocking I/O)")
	print("==========================================")

	-- Initialize PortAudio
	pa_check(pa.Pa_Initialize())

	-- Setup output parameters
	local outputParameters = ffi.new("PaStreamParameters")
	outputParameters.device = pa.Pa_GetDefaultOutputDevice()
	outputParameters.channelCount = 2 -- Stereo
	outputParameters.sampleFormat = paFloat32
	outputParameters.suggestedLatency = 0.050 -- 50ms
	outputParameters.hostApiSpecificStreamInfo = nil

	-- Open stream (no callback for blocking I/O)
	local stream = ffi.new("PaStream*[1]")
	pa_check(pa.Pa_OpenStream(
		stream,
		nil, -- No input
		outputParameters,
		SAMPLE_RATE,
		FRAMES_PER_BUFFER,
		paClipOff,
		nil, -- No callback
		nil -- No user data
	))

	-- Start stream
	pa_check(pa.Pa_StartStream(stream[0]))

	print("\nPlaying notes with ADSR envelope...")
	print("Press Ctrl+C to stop\n")

	-- Create synth components
	local oscillator = Oscillator.new(440)
	local envelope = ADSR.new(0.05, 0.1, 0.6, 0.2)
	local master_volume = 0.3

	-- Play a sequence of notes
	local notes = {
		{ freq = 261.63, duration = 500 }, -- C4
		{ freq = 329.63, duration = 500 }, -- E4
		{ freq = 392.00, duration = 500 }, -- G4
		{ freq = 523.25, duration = 800 }, -- C5
		{ freq = 392.00, duration = 500 }, -- G4
		{ freq = 329.63, duration = 500 }, -- E4
		{ freq = 261.63, duration = 800 }, -- C4
	}

	for i, note in ipairs(notes) do
		print(string.format("Playing %.2f Hz for %d ms", note.freq, note.duration))

		oscillator:set_frequency(note.freq)
		envelope:note_on()

		-- Play note
		local num_buffers = math.ceil((note.duration / 1000) * SAMPLE_RATE / FRAMES_PER_BUFFER)
		for j = 1, num_buffers do
			local buffer = generate_audio(oscillator, envelope, FRAMES_PER_BUFFER, master_volume)
			pa_check(pa.Pa_WriteStream(stream[0], buffer, FRAMES_PER_BUFFER))
		end

		envelope:note_off()

		-- Gap between notes (release)
		local gap_buffers = math.ceil(0.2 * SAMPLE_RATE / FRAMES_PER_BUFFER)
		for j = 1, gap_buffers do
			local buffer = generate_audio(oscillator, envelope, FRAMES_PER_BUFFER, master_volume)
			pa_check(pa.Pa_WriteStream(stream[0], buffer, FRAMES_PER_BUFFER))
		end
	end

	-- Cleanup
	pa_check(pa.Pa_StopStream(stream[0]))
	pa_check(pa.Pa_CloseStream(stream[0]))
	pa_check(pa.Pa_Terminate())

	print("\nDone!")
end

-- Run with error handling
local success, err = pcall(main)
if not success then
	print("Error: " .. tostring(err))
	pa.Pa_Terminate()
	os.exit(1)
end
