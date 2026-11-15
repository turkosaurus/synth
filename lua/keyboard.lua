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

-- A fretboard like layout, where each row is a string.
-- Offset represents the semitones between rows.
--   4 (major third):     SP-404 pad grid (nice for macro/numpads or ortholinear)
--   5 (perfect fourth):  bass or guitar (good for character keyboards)
--   7 (perfect fifth):   mandolin or violin (also good for character keyboards)
-- Anchor is the character for A4, around which all others are derived.
local defaults = {
	anchor = {
		key = "a", -- second string "open"
		note = 69, -- MIDI A4
	},
	offset = 5, -- semitones offset between rows
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
-- `name` is carried so a view can report which layout is in use. A patch
-- supplying its own rows leaves it unset, and reads as custom.
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
	if out.template == nil then
		out.template = keyboard.template("qwerty")
	end
	return out
end

return keyboard
