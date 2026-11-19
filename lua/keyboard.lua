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

-- `notes` maps each character to a MIDI note. Rust reads only that map, so
-- any layout works: `keyboard.grid` below, or a patch's own table. Left out,
-- it is `keyboard.grid()`. `name` is shown by the TUI; a patch's own `notes`
-- with no `name` reads as custom.
local defaults = {
	octave_up = { "=", "+" },
	octave_down = { "-", "_" },
	stop = { "escape" }, -- silence everything; must not be empty
	help = { "/", "?" },
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

-- Grid of character keys in their lowercase and shifted forms.
keyboard.templates = {
	qwerty = {
		name = "qwerty",
		rows = { "zxcvbnm,.", "asdfghjkl;", "qwertyuiop", "1234567890" },
		shifted = { "ZXCVBNM<>", "ASDFGHJKL:", "QWERTYUIOP", "!@#$%^&*()" },
	},
	dvorak = {
		name = "dvorak",
		rows = { "qjkxbmwvz", "aoeuidhtns", "pyfgcrl", "1234567890" },
		shifted = { "QJKXBMWVZ", "AOEUIDHTNS", "PYFGCRL", "!@#$%^&*()" },
	},
}

function keyboard.template(name)
	local template = keyboard.templates[name]
	if not template then
		error("keyboard.template: unknown template '" .. tostring(name) .. "'", 2)
	end
	return template
end

-- A fretboard like layout, where each row is a string.
-- Offset represents the semitones between rows.
--   4 (major third):     SP-404 pad grid (nice for macro/numpads or ortholinear)
--   5 (perfect fourth):  bass or guitar (good for character keyboards)
--   7 (perfect fifth):   mandolin or violin (also good for character keyboards)
-- Anchor is the character for A4, around which all others are derived.
local grid_defaults = {
	anchor = {
		key = "a", -- second string "open"
		note = 69, -- MIDI A4
	},
	offset = 5, -- semitones offset between rows
	template = keyboard.templates.qwerty,
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

-- The fretboard as a `notes` map: along a row each key is a semitone up,
-- each row is `offset` above the one below it, and a shifted key is its
-- plain key an octave up. Keys that land outside MIDI's 0 to 127 are left
-- out.
function keyboard.grid(opts)
	opts = opts or {}
	for key in pairs(opts) do
		if grid_defaults[key] == nil then
			error("keyboard.grid: unknown field '" .. tostring(key) .. "'", 2)
		end
	end
	local anchor = opts.anchor or grid_defaults.anchor
	local offset = opts.offset or grid_defaults.offset
	local template = opts.template or grid_defaults.template

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
			if ch == anchor.key then
				base = anchor.note - (r - 1) * offset - (c - 1)
			end
		end
	end
	if base == nil then
		error("keyboard.grid: anchor key '" .. tostring(anchor.key) .. "' does not appear in the template", 2)
	end

	local notes = {}
	for r = 1, #rows do
		for c = 1, #rows[r] do
			local note = base + (r - 1) * offset + (c - 1)
			if note >= 0 and note <= 127 then
				notes[rows[r][c]] = note
			end
			if note + 12 >= 0 and note + 12 <= 127 then
				notes[shifted[r][c]] = note + 12
			end
		end
	end
	return notes
end

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
	if out.notes == nil then
		out.notes = keyboard.grid()
		if out.name == nil then
			out.name = grid_defaults.template.name
		end
	end
	return out
end

return keyboard
