package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local function assert_config_error(configured_options, pattern)
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_plugin(wezterm)
  local ok, err = pcall(wisp.apply_to_config, {}, configured_options)
  assert(not ok, "invalid options should fail")
  assert(tostring(err):match(pattern), "configuration error should mention " .. pattern .. ": " .. tostring(err))
end

helper.test("cache TTL must be a non-negative number", function()
  assert_config_error({ cache_ttl_seconds = -1 }, "cache_ttl_seconds")
  assert_config_error({ cache_ttl_seconds = "60" }, "cache_ttl_seconds")
end)

helper.test("roots and fixed projects require paths", function()
  assert_config_error({ roots = { {} } }, "roots%[1%]%.path")
  assert_config_error({ projects = { { name = "missing" } } }, "projects%[1%]%.path")
end)

helper.test("file opener argv must contain strings", function()
  assert_config_error({ open_file = {} }, "open_file")
  assert_config_error({ open_file = { "nvim", false } }, "open_file%[2%]")
end)

helper.test("project spawn domains must use a stable domain name", function()
  assert_config_error({ spawn_domain = "DefaultDomain" }, "spawn_domain")
  assert_config_error({ roots = { { path = "~/Repos", domain = "CurrentPaneDomain" } } }, "roots%[1%]%.domain")
  assert_config_error({ projects = { { path = "~/api", domain = { DomainId = 1 } } } }, "projects%[1%]%.domain")
end)
