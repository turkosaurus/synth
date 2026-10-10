#!/usr/bin/env luajit

local sequence = {}

-- `note` and `voice` stick until a later step changes them, so a step only
-- names what differs. Resolved here so Rust reads a uniform list.
local function resolve(pattern, into, default_voice)
	-- a fresh copy per step: a pattern used twice
	-- must not inherit the voice its first appearance resolved to
	local voice = default_voice
	for i = 1, #pattern do
		local step = pattern[i]
		voice = step.voice or voice

		-- copied field by field rather than rebuilt: listing the known keys
		-- here would turn a misspelled one into a silent rest
		local out = {}
		for key, value in pairs(step) do
			out[key] = value
		end
		out.voice = voice
		out.duration = step.duration or 0.5 -- steps to sound; may exceed 1
		into[#into + 1] = out
	end
end

-- Build a list from a function body. A table constructor cannot hold
-- statements, so this is how a loop stays inline where its result is used
-- instead of being hoisted above the table.
function sequence.list(build)
	local out = {}
	build(function(...)
		for i = 1, select("#", ...) do
			out[#out + 1] = select(i, ...)
		end
	end)
	return out
end

-- `patterns` goes in as a table and comes out as a count, so unlike the
-- other constructors this cannot pass opts through. The check lives here
-- instead.
local known = {
	bpm = true,
	division = true,
	voice = true,
	voices = true,
	patterns = true,
	chain = true,
	loop = true,
	steps = true,
	keyboard = true,
}

-- Either a flat `steps` list, or `patterns` named and arranged by `chain`.
-- Patterns are flattened here, so Rust only ever sees one step list.
function sequence.new(opts)
	for key in pairs(opts) do
		if not known[key] then
			error("sequence.new: unknown field '" .. tostring(key) .. "'", 2)
		end
	end

	local steps = {}
	local patterns = 0
	local chain = opts.chain or {}
	local loop = opts.loop or 1

	if opts.patterns then
		for _ in pairs(opts.patterns) do
			patterns = patterns + 1
		end
		for _ = 1, loop do
			for _, name in ipairs(chain) do
				local pattern = opts.patterns[name]
				if not pattern then
					error("chain names pattern '" .. tostring(name) .. "', which is not defined")
				end
				resolve(pattern, steps, opts.voice)
			end
		end
	else
		resolve(opts.steps or {}, steps, opts.voice)
	end

	return {
		bpm = opts.bpm or 120,
		division = opts.division or 1, -- steps per beat; 4 = sixteenths
		voices = opts.voices or {},
		steps = steps,
		patterns = patterns,
		chain = #chain,
		loop = loop,
		keyboard = opts.keyboard, -- a sequence may ship its own layout
	}
end

return sequence
