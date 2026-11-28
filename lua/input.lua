local construct = require("synth.construct")

local defaults, override -- defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.input
---@overload fun(opts?: synth.Keyboard): synth.Keyboard
local input = setmetatable({}, {
	__call = function(_, opts)
		return override(construct("keyboard", site(), opts, defaults))
	end,
})

-- Physical rows of a character keyboard, bottom to top, each in its plain
-- and shifted forms. Every template lines its rows up on the same keys, so
-- one position plays the same note whatever the layout. `anchor` is the key
-- in the home row's first position.
--
-- The octave and help keys are the layout's own, so none of them falls in
-- its note rows. Where qwerty's spot holds a letter or a dead key, a nearby
-- symbol key stands in.
---@class synth.Template
---@field name string
---@field anchor string
---@field rows string[]
---@field shifted string[]
---@field octave_down string[]
---@field octave_up string[]
---@field help string[]

-- What Rust reads as a keyboard's `layout`: `notes` maps each character to
-- a MIDI note.
---@class synth.Layout
---@field name string
---@field notes table<string, integer>
---@field octave_up string[]
---@field octave_down string[]
---@field help string[] may be empty

input.templates = {
	qwerty = {
		name = "qwerty",
		anchor = "a",
		rows = { "zxcvbnm,.", "asdfghjkl;", "qwertyuiop", "1234567890" },
		shifted = { "ZXCVBNM<>", "ASDFGHJKL:", "QWERTYUIOP", "!@#$%^&*()" },
		octave_down = { "-", "_" },
		octave_up = { "=", "+" },
		help = { "/", "?" },
	},
	qwertz = {
		name = "qwertz",
		anchor = "a",
		rows = { "yxcvbnm,.", "asdfghjklö", "qwertzuiop", "1234567890" },
		shifted = { "YXCVBNM;:", "ASDFGHJKLÖ", "QWERTZUIOP", '!"§$%&/()=' },
		octave_down = { "ß", "?" },
		-- right of ü: the key right of ß is a dead accent
		octave_up = { "+", "*" },
		help = { "-", "_" },
	},
	azerty = {
		name = "azerty",
		anchor = "q",
		rows = { "wxcvbn,;:", "qsdfghjklm", "azertyuiop", "&é\"'(-è_çà" },
		shifted = { "WXCVBN?./", "QSDFGHJKLM", "AZERTYUIOP", "1234567890" },
		octave_down = { ")", "°" },
		octave_up = { "=", "+" },
		help = { "!", "§" },
	},
	dvorak = {
		name = "dvorak",
		anchor = "a",
		rows = { ";qjkxbmwv", "aoeuidhtns", "',.pyfgcrl", "1234567890" },
		shifted = { ":QJKXBMWV", "AOEUIDHTNS", '"<>PYFGCRL', "!@#$%^&*()" },
		octave_down = { "[", "{" },
		octave_up = { "]", "}" },
		-- qwerty's spot is z
		help = { "/", "?" },
	},
	colemak = {
		name = "colemak",
		anchor = "a",
		rows = { "zxcvbkm,.", "arstdhneio", "qwfpgjluy;", "1234567890" },
		shifted = { "ZXCVBKM<>", "ARSTDHNEIO", "QWFPGJLUY:", "!@#$%^&*()" },
		octave_down = { "-", "_" },
		octave_up = { "=", "+" },
		help = { "/", "?" },
	},
}

-- A fretboard like layout, where each row is a string.
-- Offset represents the semitones between rows.
--   4 (major third):     SP-404 pad grid (nice for macro/numpads or ortholinear)
--   5 (perfect fourth):  bass or guitar (good for character keyboards)
--   7 (perfect fifth):   mandolin or violin (also good for character keyboards)
-- Note is the MIDI note of the template's anchor key, around which all others
-- are derived.
---@class synth.Grid
---@field template? synth.Template
---@field offset? integer semitones offset between rows
---@field note? integer MIDI note of the template's anchor key

local grid_defaults = {
	template = input.templates.qwerty,
	offset = 5, -- semitones offset between rows
	note = 69, -- MIDI A4, on the second string "open"
}

-- The characters of a UTF-8 string. LuaJIT has no utf8 library, and a
-- template row may hold a key outside ASCII.
local function chars(text)
	local out = {}
	for ch in text:gmatch("[%z\1-\127\194-\244][\128-\191]*") do
		out[#out + 1] = ch
	end
	return out
end

-- A layout from a template laid out as a fretboard, carrying the template's
-- name and control keys. Along a row each key is a semitone up, each row is
-- `offset` above the one below it, and a shifted key is its plain key an
-- octave up. Keys that land outside MIDI's 0 to 127 are left out.
---@param opts? synth.Grid
---@return synth.Layout
function input.grid(opts)
	opts = opts or {}
	for key in pairs(opts) do
		if grid_defaults[key] == nil then
			error("input.grid: unknown field '" .. tostring(key) .. "'", 2)
		end
	end
	local template = opts.template or grid_defaults.template
	local offset = opts.offset or grid_defaults.offset
	local note = opts.note or grid_defaults.note

	if #template.rows ~= #template.shifted then
		error("input.grid: a template's rows and shifted must have the same number of rows", 2)
	end
	local rows, shifted = {}, {}
	for r = 1, #template.rows do
		rows[r] = chars(template.rows[r])
		shifted[r] = chars(template.shifted[r])
		if #rows[r] ~= #shifted[r] then
			error("input.grid: template row " .. r .. " and its shifted form differ in length", 2)
		end
	end

	-- the anchor fixes the grid, so changing `offset` keeps that key on pitch
	local base
	for r = 1, #rows do
		for c, ch in ipairs(rows[r]) do
			if ch == template.anchor then
				base = note - (r - 1) * offset - (c - 1)
			end
		end
	end
	if base == nil then
		error("input.grid: anchor key '" .. tostring(template.anchor) .. "' does not appear in the template", 2)
	end

	local notes = {}
	local function put(ch, n)
		if n >= 0 and n <= 127 then
			notes[ch] = n
		end
	end
	for r = 1, #rows do
		for c = 1, #rows[r] do
			local n = base + (r - 1) * offset + (c - 1)
			put(rows[r][c], n)
			put(shifted[r][c], n + 12)
		end
	end
	return {
		name = template.name,
		notes = notes,
		octave_up = template.octave_up,
		octave_down = template.octave_down,
		help = template.help,
	}
end

-- Every template as a ready layout, for `input({ layout = ... })`.
-- Spelled out rather than looped so editors can complete the names.
input.qwerty = input.grid({ template = input.templates.qwerty })
input.qwertz = input.grid({ template = input.templates.qwertz })
input.azerty = input.grid({ template = input.templates.azerty })
input.dvorak = input.grid({ template = input.templates.dvorak })
input.colemak = input.grid({ template = input.templates.colemak })

-- `octave_up`, `octave_down`, and `help` replace the layout's.
---@class synth.Keyboard
---@field layout? synth.Layout
---@field stop? string[] silence everything; must not be empty
---@field scope? string[] move the scope to the next channel, mix, or stream; may be empty
---@field quit? string[] quit after a y/n prompt
---@field quit_now? string[] quit without asking
---@field gate? number ms; note length when key release is unreported
---@field autorepeat_threshold? number ms; a press this soon is autorepeat; 0 off
---@field doubletap_threshold? number ms; a second press this soon is a double tap; 0 off
---@field velocity? integer 1 to 127, for every note
---@field octave? integer the octave shift to start at
---@field octave_low? integer octaves octave_down may move from the start
---@field octave_high? integer octaves octave_up may move from the start
---@field octave_up? string[]
---@field octave_down? string[]
---@field help? string[]
defaults = {
	-- `{ name, notes, octave_up, octave_down, help }`, where `notes` maps
	-- each character to a MIDI note. Rust reads only this table, so a patch
	-- may write its own. `help` may be empty. `octave_up`, `octave_down`, and
	-- `help` given on the keyboard itself replace the layout's.
	layout = input.qwerty,
	stop = { "escape" }, -- silence everything; must not be empty
	scope = { "tab" }, -- move the scope to the next channel, mix, or stream; may be empty
	-- either may be empty, not both; "ctrl-c" names a key pressed with ctrl
	quit = {}, -- quit after a y/n prompt
	quit_now = { "ctrl-c", "ctrl-d" }, -- quit without asking
	-- note length when the terminal cannot report key release
	gate = 200, -- ms
	-- when the terminal cannot report key release, a press of a key this
	-- soon after its last press, repeats included, is taken as OS
	-- autorepeat and ignored. Longer than the OS repeat interval (GNOME
	-- 30ms, X11 40ms), shorter than a hand repeats one key: a held key
	-- sounds once, then once more at the OS repeat delay. 0 turns it off.
	autorepeat_threshold = 60, -- ms
	-- a second press of a key this soon after the first is a double tap.
	-- The first press acts at once and never waits for a second. A double
	-- tap of stop cuts the sound, skipping the release. 0 to 1000; 0 off.
	doubletap_threshold = 400, -- ms
	-- a key has no depth to time, so every note gets this velocity; 1 to 127
	velocity = 100,
	-- the octave shift to start at; 0 plays the layout as written
	octave = -1,
	-- how many octaves octave_down and octave_up may move from the start
	octave_low = 2,
	octave_high = 3,
}

-- `octave_up`, `octave_down`, and `help` on the keyboard replace the
-- layout's. The layout is copied first, so a shared one such as
-- `input.qwerty` passed by the caller is left alone.
override = function(out)
	local layout
	for _, key in ipairs({ "octave_up", "octave_down", "help" }) do
		-- a layout that is not a table is left for Rust to name
		if out[key] ~= nil and type(out.layout) == "table" then
			if layout == nil then
				layout = {}
				for k, v in pairs(out.layout) do
					layout[k] = v
				end
			end
			layout[key] = out[key]
			out[key] = nil
		end
	end
	out.layout = layout or out.layout
	return out
end

return input
