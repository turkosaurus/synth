-- A pattern of your own: a list of steps. A sequence uses it with
--   patterns = { intro = require("pattern.example") }
-- Each step is one grid slot; an empty step is a rest.
return {
	{ note = 60 },
	{ note = 63, duration = 0.5 }, -- steps to sound; may exceed 1
	{},
	{ note = 67 },
	-- { note = 70, instrument = "lead" }, -- names an instrument of the sequence
}
