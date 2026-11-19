local new -- the constructor, defined below

-- `file:line` of whoever called the constructor, read by Rust
local site = require("synth.site")

---@class synth.keyboard
---@overload fun(opts?: table): table
local keyboard = setmetatable({}, {
	__call = function(_, opts)
		return setmetatable(new(opts), { kind = "keyboard", at = site() })
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
keyboard.templates = {
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
local grid_defaults = {
	template = keyboard.templates.qwerty,
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
-- name and control keys: along a row each key is
-- a semitone up, each row is `offset` above the one below it, and a shifted
-- key is its plain key an octave up. Keys that land outside MIDI's 0 to 127
-- are left out.
function keyboard.grid(opts)
	opts = opts or {}
	for key in pairs(opts) do
		if grid_defaults[key] == nil then
			error("keyboard.grid: unknown field '" .. tostring(key) .. "'", 2)
		end
	end
	local template = opts.template or grid_defaults.template
	local offset = opts.offset or grid_defaults.offset
	local note = opts.note or grid_defaults.note

	if #template.rows ~= #template.shifted then
		error("keyboard.grid: a template's rows and shifted must have the same number of rows", 2)
	end
	local rows, shifted = {}, {}
	for r = 1, #template.rows do
		rows[r] = chars(template.rows[r])
		shifted[r] = chars(template.shifted[r])
		if #rows[r] ~= #shifted[r] then
			error("keyboard.grid: template row " .. r .. " and its shifted form differ in length", 2)
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
		error("keyboard.grid: anchor key '" .. tostring(template.anchor) .. "' does not appear in the template", 2)
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

-- Every template as a ready layout, for `keyboard({ layout = ... })`.
-- Spelled out rather than looped so editors can complete the names.
keyboard.qwerty = keyboard.grid({ template = keyboard.templates.qwerty })
keyboard.qwertz = keyboard.grid({ template = keyboard.templates.qwertz })
keyboard.azerty = keyboard.grid({ template = keyboard.templates.azerty })
keyboard.dvorak = keyboard.grid({ template = keyboard.templates.dvorak })
keyboard.colemak = keyboard.grid({ template = keyboard.templates.colemak })

local defaults = {
	-- `{ name, notes, octave_up, octave_down, help }`, where `notes` maps
	-- each character to a MIDI note. Rust reads only this table, so a patch
	-- may write its own. `help` may be empty.
	layout = keyboard.qwerty,
	stop = { "escape" }, -- silence everything; must not be empty
	-- note length when the terminal cannot report key release
	gate = 200, -- ms
	-- when the terminal cannot report key release, a press of a key this
	-- soon after its last press is taken as OS autorepeat and ignored. It
	-- must outlast the OS repeat delay (GNOME defaults to 500ms, X11 to
	-- 660ms), and one key cannot retrigger faster than this. 0 turns it off.
	repeat_window = 550, -- ms
	-- a key has no depth to time, so every note gets this velocity; 1 to 127
	velocity = 100,
	-- how many octaves octave_up and octave_down may move from the start
	octave_range = 4,
}

-- A new table: the caller's fields, then defaults for any left out. The
-- caller's table is left alone, and nothing is discarded, so a misspelled
-- field reaches Rust and gets named instead of silently taking a default.
new = function(opts)
	local out = {}
	for key, value in pairs(opts or {}) do
		out[key] = value
	end
	for key, default in pairs(defaults) do
		if out[key] == nil then
			out[key] = default
		end
	end
	return out
end

return keyboard
