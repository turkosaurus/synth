-- Checks what a Neovim user sees from lua-language-server in a patches
-- folder: the root it finds, hover, and completion.
-- Run by `lint:lsp` as `nvim --headless --clean -l tests/nvim/lsp.lua <patches>`.

local patches = assert(arg[1], "usage: lsp.lua <patches folder>")
local failures = {}

local function fail(message)
	failures[#failures + 1] = message
end

-- A patch written for the probe, removed at the end: line and column are
-- 0-based, as LSP counts them.
local probe = patches .. "/probe.lua"
local lines = {
	"local synth = require('synth')",
	"local env = synth.envelope({  })",
	"local cut = synth.filter.",
}
vim.fn.writefile(lines, probe)

local root = vim.fs.root(probe, { ".luarc.json" })
if root ~= vim.fs.normalize(patches) then
	fail(("root from .luarc.json: want %s, got %s"):format(patches, tostring(root)))
end

vim.cmd.edit(probe)
local buf = vim.api.nvim_get_current_buf()
local id = vim.lsp.start({
	name = "lua_ls",
	cmd = { "lua-language-server" },
	root_dir = root or patches,
}, { bufnr = buf })
local client = assert(vim.lsp.get_client_by_id(assert(id, "lua_ls did not start")))

local function params(line, character)
	return {
		textDocument = vim.lsp.util.make_text_document_params(buf),
		position = { line = line, character = character },
	}
end

-- The server answers before it has read the workspace, so each question is
-- asked until its answer passes or the time runs out.
local function ask(method, line, character, check)
	local last
	local ok = vim.wait(30000, function()
		local reply = client:request_sync(method, params(line, character), 5000, buf)
		last = reply and reply.result
		return last ~= nil and check(last)
	end, 500)
	return ok, last
end

local function labels(result)
	local items = result.items or result
	local out = {}
	for _, item in ipairs(items) do
		-- an optional field is labelled `attack_curve?`
		out[(item.label:gsub("%?$", ""))] = true
	end
	return out
end

local function hover_text(result)
	local contents = result.contents
	return type(contents) == "table" and (contents.value or vim.inspect(contents)) or tostring(contents)
end

local ok, result = ask("textDocument/hover", 1, 20, function(r)
	return hover_text(r):find("synth.Envelope", 1, true) ~= nil
end)
if not ok then
	fail("hover on synth.envelope does not name synth.Envelope: " .. (result and hover_text(result) or "no reply"))
end

ok = ask("textDocument/completion", 1, 29, function(r)
	return labels(r).attack_curve ~= nil
end)
if not ok then
	fail("completion in synth.envelope({ }) does not offer attack_curve")
end

local filters = { "lowpass", "highpass", "bandpass", "notch", "bell", "low_shelf", "high_shelf" }
ok, result = ask("textDocument/completion", 2, 25, function(r)
	local have = labels(r)
	for _, name in ipairs(filters) do
		if not have[name] then
			return false
		end
	end
	return true
end)
if not ok then
	fail("completion after synth.filter. does not offer every filter")
end

client:stop(true)
os.remove(probe)

if #failures > 0 then
	for _, message in ipairs(failures) do
		io.stderr:write("nvim: " .. message .. "\n")
	end
	os.exit(1)
end
print("nvim: root, hover, and completion as expected")
