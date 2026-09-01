package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

local function assert_config_error(configured_options, pattern)
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  local ok, err = pcall(wisp.apply_to_config, {}, configured_options)
  assert(not ok, "invalid options should fail")
  assert(tostring(err):match(pattern), "configuration error should mention " .. pattern .. ": " .. tostring(err))
end

helper.test("former filesystem options have no legacy handling", function()
  assert_config_error({ roots = {} }, "unknown option roots")
  assert_config_error({ projects = {} }, "unknown option projects")
  assert_config_error({ open_file = { "nvim" } }, "unknown option open_file")
end)

helper.test("executable and config paths must be non-empty strings", function()
  assert_config_error({ wisp_path = "/tmp/wisp" }, "unknown option wisp_path")
  assert_config_error({ config_file = false }, "config_file")
end)

helper.test("poll timing must be positive", function()
  assert_config_error({ poll_interval_seconds = 0 }, "poll_interval_seconds")
  assert_config_error({ picker_timeout_seconds = "60" }, "picker_timeout_seconds")
end)

helper.test("window preview initial visibility is a boolean", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { window_preview = true })
  wisp.apply_to_config({}, { window_preview = false })
  assert_config_error({ window_preview = "yes" }, "window_preview")
end)

helper.test("single-pane behavior is strict and defaults to showing panes", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { single_pane_behavior = "show" })
  wisp.apply_to_config({}, { single_pane_behavior = "activate" })
  assert_config_error({ single_pane_behavior = "skip" }, "single_pane_behavior")
end)

helper.test("status providers and popup options are strict", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {
    status_items = {
      { name = "opencode", action = "sessions" },
      { name = "directory", action = "projects" },
    },
    popup = { direction = "Bottom", size = 0.65 },
  })

  assert_config_error({ status_items = { { name = "missing" } } }, "status_items")
  assert_config_error({ status_items = { { name = "directory", action = "missing" } } }, "status_items")
  assert_config_error({ status_items = { { name = "directory", unknown = true } } }, "status_items")
  local sparse = { { name = "opencode" }, { name = "opencode" }, { name = "directory" } }
  sparse[2] = nil
  assert_config_error({ status_items = sparse }, "dense")
  assert_config_error({ popup = { direction = "Center", size = 0.5 } }, "popup")
  assert_config_error({ popup = { direction = "Bottom", size = 0 } }, "popup")
  assert_config_error({ popup = { direction = "Bottom", unknown = true } }, "popup")
end)

helper.test("spawn and picker domains must use stable domain names", function()
  assert_config_error({ spawn_domain = "DefaultDomain" }, "spawn_domain")
  assert_config_error({ picker_domain = { DomainId = 1 } }, "picker_domain")
end)

helper.test("project policy hooks must be functions", function()
  assert_config_error({ workspace_for_project = "wisp" }, "workspace_for_project")
  assert_config_error({ domain_for_project = {} }, "domain_for_project")
end)

helper.test("file open target is strict and defaults to window", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  for _, target in ipairs { "window", "right_pane", "bottom_pane" } do
    wisp.apply_to_config({}, { file_open = { default = target } })
  end

  assert_config_error({ file_open = "window" }, "file_open")
  assert_config_error({ file_open = {} }, "file_open")
  assert_config_error({ file_open = { default = "tab" } }, "file_open")
  assert_config_error({ file_open = { default = "window", unknown = true } }, "file_open")
end)

helper.test("file preview requires an exact command direction and size", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {
    file_preview = { command = { "nvim", "--clean" }, direction = "Right", size = 0.5 },
  })

  assert_config_error({ file_preview = {} }, "file_preview")
  assert_config_error({ file_preview = { command = {}, direction = "Right", size = 0.5 } }, "command")
  assert_config_error({ file_preview = { command = { "nvim", "" }, direction = "Right", size = 0.5 } }, "command")
  local sparse = { "nvim", "--clean" }
  sparse[1] = nil
  assert_config_error({ file_preview = { command = sparse, direction = "Right", size = 0.5 } }, "command")
  assert_config_error({ file_preview = { command = { "nvim" }, direction = "Center", size = 0.5 } }, "direction")
  assert_config_error({ file_preview = { command = { "nvim" }, direction = "Right", size = 0 } }, "size")
  assert_config_error(
    { file_preview = { command = { "nvim" }, direction = "Right", size = 0.5, script = "x" } },
    "unknown"
  )
end)
