local construct = require("synth.construct")

local new, defaults, step_defaults -- defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.sequence
---@overload fun(opts: synth.SequenceOpts): synth.Sequence
local sequence = setmetatable({}, {
	__call = function(_, opts)
		local filled = construct("sequence", site(), opts, defaults)
		-- the flattened result is a new table, so it takes the stamp over
		return setmetatable(new(filled), getmetatable(filled))
	end,
})

-- `note` and `instrument` stick until a later step changes them.
---@class synth.Step
---@field note? integer MIDI note, 0 to 127
---@field off? boolean
---@field instrument? string a key of the sequence's `instruments`
---@field duration? number steps to sound; may exceed 1
---@field velocity? integer 1 to 127

-- Either a flat `steps` list, or `patterns` named and arranged by `chain`.
---@class synth.SequenceOpts
---@field bpm? number
---@field division? number steps per beat; 4 = sixteenths
---@field instrument? string the instrument steps start with
---@field instruments? table<string, synth.Instrument>
---@field steps? synth.Step[]
---@field patterns? table<string, synth.Step[]>
---@field chain? string[] pattern names, in play order
---@field loop? integer times to play the chain
---@field keyboard? synth.Keyboard
---@field tuning? synth.Tuning
---@field filter? synth.Filter runs on the summed output

-- What Rust reads: patterns flattened into one step list.
---@class synth.Sequence
---@field bpm number
---@field division number
---@field instruments table<string, synth.Instrument>
---@field steps synth.Step[]
---@field patterns integer how many patterns were defined
---@field chain integer how many names the chain held
---@field loop integer
---@field keyboard? synth.Keyboard
---@field tuning? synth.Tuning
---@field filter? synth.Filter

defaults = {
	bpm = 120,
	division = 1, -- steps per beat; 4 = sixteenths
	loop = 1, -- times to play the chain
}

step_defaults = {
	duration = 0.5, -- steps to sound; may exceed 1
	velocity = 127, -- 1 to 127
}

-- `note` and `instrument` stick until a later step changes them,
-- so a step only names what differs.
-- Resolved here so Rust reads a uniform list.
local function resolve(pattern, into, default_instrument)
	-- a fresh copy per step: a pattern used twice
	-- must not inherit the instrument its first appearance resolved to
	local instrument = default_instrument
	for i = 1, #pattern do
		local step = pattern[i]
		instrument = step.instrument or instrument

		-- copied field by field rather than rebuilt: listing the known keys
		-- here would turn a misspelled one into a silent rest
		local out = {}
		for key, value in pairs(step) do
			out[key] = value
		end
		out.instrument = instrument
		for key, default in pairs(step_defaults) do
			if out[key] == nil then
				out[key] = default
			end
		end
		into[#into + 1] = out
	end
end

-- A table constructor cannot hold statements, so this makes a function body
-- an expression, letting a loop build a list inline where it is used.
---@generic T
---@param build fun(add: fun(...: T))
---@return T[]
function sequence.list(build)
	local out = {}
	build(function(...)
		for i = 1, select("#", ...) do
			out[#out + 1] = select(i, ...)
		end
	end)
	return out
end

-- The other constructors copy every field they are given, so an unknown
-- one reaches Rust and gets named there. sequence builds its result from
-- scratch and would drop an unknown field, so it checks them here.
local known = {
	bpm = true,
	division = true,
	instrument = true,
	instruments = true,
	patterns = true,
	chain = true,
	loop = true,
	steps = true,
	keyboard = true,
	filter = true,
	tuning = true,
}

-- Either a flat `steps` list, or `patterns` named and arranged by `chain`.
-- Patterns are flattened here, so Rust only ever sees one step list.
new = function(opts)
	for key in pairs(opts) do
		if not known[key] then
			error("sequence: unknown field '" .. tostring(key) .. "'", 2)
		end
	end

	local steps = {}
	local patterns = 0
	local chain = opts.chain or {}
	local loop = opts.loop

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
				resolve(pattern, steps, opts.instrument)
			end
		end
	else
		resolve(opts.steps or {}, steps, opts.instrument)
	end

	return {
		bpm = opts.bpm,
		division = opts.division,
		instruments = opts.instruments or {},
		steps = steps,
		patterns = patterns,
		chain = #chain,
		loop = loop,
		keyboard = opts.keyboard, -- a sequence may ship its own layout
		tuning = opts.tuning, -- and its own tuning
		filter = opts.filter, -- runs on the summed output
	}
end

return sequence
