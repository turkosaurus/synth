-- Every device a patch can play from, one module each.
---@class synth.input
local input = {
	keyboard = require("synth.input.keyboard"),
}

-- A patch's `input` field: one entry per device it plays from.
---@class synth.Input
---@field keyboard? synth.Keyboard

return input
