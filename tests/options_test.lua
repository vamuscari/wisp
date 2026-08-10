package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local function assert_config_error(configured_options, pattern)
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_plugin(wezterm)
  local ok, err = pcall(wisp.apply_to_config, {}, configured_options)
  assert(not ok, "invalid options should fail")
  assert(tostring(err):match(pattern), "configuration error should mention " .. pattern .. ": " .. tostring(err))
end

helper.test("filesystem options moved to shared TOML", function()
  assert_config_error({ roots = {} }, "shared Wisp TOML")
  assert_config_error({ projects = {} }, "shared Wisp TOML")
  assert_config_error({ open_file = { "nvim" } }, "shared Wisp TOML")
end)

helper.test("executable and config paths must be non-empty strings", function()
  assert_config_error({ wisp_path = "" }, "wisp_path")
  assert_config_error({ config_file = false }, "config_file")
end)

helper.test("poll timing must be positive", function()
  assert_config_error({ poll_interval_seconds = 0 }, "poll_interval_seconds")
  assert_config_error({ picker_timeout_seconds = "60" }, "picker_timeout_seconds")
end)

helper.test("spawn and picker domains must use stable domain names", function()
  assert_config_error({ spawn_domain = "DefaultDomain" }, "spawn_domain")
  assert_config_error({ picker_domain = { DomainId = 1 } }, "picker_domain")
end)

helper.test("project policy hooks must be functions", function()
  assert_config_error({ workspace_for_project = "wisp" }, "workspace_for_project")
  assert_config_error({ domain_for_project = {} }, "domain_for_project")
end)
