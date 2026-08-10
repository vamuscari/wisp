local tests = {
  "tests/plugin_api_test.lua",
  "tests/options_test.lua",
  "tests/refresh_test.lua",
  "tests/process_adapter_test.lua",
  "tests/workspace_action_test.lua",
  "tests/tab_split_test.lua",
  "tests/nvim_adapter_test.lua",
}

local script = arg and arg[0] or "tests/run.lua"
local directory = script:match "^(.*[/\\])" or ""
local root = directory:gsub("tests[/\\]$", "")
package.path = root .. "?.lua;" .. root .. "?/init.lua;" .. package.path

for _, test in ipairs(tests) do
  dofile(root .. test)
end

print "all tests passed"
