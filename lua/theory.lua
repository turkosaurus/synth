#!/usr/bin/env luajit

local theory = {}

local letters = { "A", "", "B", "C", "", "D", "", "E", "F", "", "G", "" }

local modifiers = {
	{ offset = 1, char = "♯", name = "sharp" },
	{ offset = 2, char = "♯♯", name = "double sharp" },
	{ offset = -1, char = "♭", name = "flat" },
	{ offset = -2, char = "♭♭", name = "double flat" },
}

local modes = {
	{
		mode = "ionian",
		nickname = "major",
		notes = { 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1 },
	},
	{
		mode = "dorian",
		nickname = "halo",
		notes = { 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 1, 0 },
	},
	{
		mode = "phrygian",
		nickname = "dark",
		offsets = { 0, -1, -1, 0, 0, -1, -1, 0 },
	},
	{
		mode = "lydian",
		nickname = "bright",
		offsets = { 0, 0, 0, 1, 0, 0, 0, 0 },
	},
	{
		mode = "mixolydian",
		nickname = "blues",
		offsets = { 0, 0, 0, 0, 0, 0, -1, 0 },
	},
	{
		mode = "aeolian",
		nickname = "minor",
		offsets = { 0, 0, -1, 0, 0, -1, -1, 0 },
	},
	{
		mode = "locrian",
		nickname = "metal",
		offsets = { 0, -1, -1, 0, -1, -1, -1, 0 },
	},
}
