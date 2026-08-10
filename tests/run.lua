local tests = {
  "tests/plugin_api_test.lua",
  "tests/options_test.lua",
  "tests/discovery_cache_test.lua",
  "tests/path_identity_test.lua",
  "tests/refresh_test.lua",
  "tests/navigation_test.lua",
  "tests/workspace_action_test.lua",
  "tests/file_open_test.lua",
  "tests/tab_split_test.lua",
}

local script = arg and arg[0] or "tests/run.lua"
local directory = script:match "^(.*[/\\])" or ""
local root = directory:gsub("tests[/\\]$", "")
package.path = root .. "?.lua;" .. root .. "?/init.lua;" .. package.path

for _, test in ipairs(tests) do
  dofile(root .. test)
end

print "all tests passed"
