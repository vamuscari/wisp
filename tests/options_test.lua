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

helper.test("OpenCode tab colors are opt-in and require a boolean", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { opencode_tab_colors = true })
  wisp.apply_to_config({}, { opencode_tab_colors = false })
  assert_config_error({ opencode_tab_colors = "yes" }, "opencode_tab_colors")
  assert_config_error({ opencode_tab_colors = true, status_bar = false }, "requires status_bar")
end)

helper.test("tab button pickers are opt-in and require a boolean", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  local config = {}
  wisp.apply_to_config(config, {})
  helper.assert_equal(wezterm.events["new-tab-button-click"], nil, "default button handler")
  wisp.apply_to_config(config, { tab_button_pickers = true })
  assert(wezterm.events["new-tab-button-click"], "enabled button handler")
  helper.assert_equal(config.show_new_tab_button_in_tab_bar, true, "visible button")
  assert_config_error({ tab_button_pickers = "yes" }, "tab_button_pickers")
end)

helper.test("Powerline tab and status surfaces accept preset and custom shapes", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)

  for _, shape in ipairs { "arrow", "slash", "slant", "rounded", "plain" } do
    wisp.apply_to_config({}, { powerline = { tabs = { shape = shape } } })
  end

  wisp.apply_to_config({}, {
    powerline = {
      tabs = {
        shape = "slant",
        gap = 1,
        padding = 1,
      },
      status = {
        shape = { left = "<", right = ">" },
        gap = 2,
        padding = 0,
      },
    },
  })
end)

helper.test("Powerline options reject malformed surfaces shapes spacing and colors", function()
  assert_config_error({ powerline = true }, "powerline must be a table")
  assert_config_error({ powerline = {} }, "tabs or status")
  assert_config_error({ powerline = { unknown = {} } }, "unknown field unknown")
  assert_config_error({ powerline = { tabs = false } }, "powerline tabs must be a table")
  assert_config_error({ powerline = { tabs = { unknown = true } } }, "tabs contains unknown field unknown")
  assert_config_error({ powerline = { tabs = { shape = "missing" } } }, "shape")
  assert_config_error({ powerline = { tabs = { shape = { left = "<" } } } }, "right")
  assert_config_error({ powerline = { tabs = { shape = { left = "<<", right = ">" } } } }, "one column")
  assert_config_error({ powerline = { tabs = { gap = -1 } } }, "gap")
  assert_config_error({ powerline = { tabs = { padding = 0.5 } } }, "padding")
  assert_config_error({ powerline = { tabs = { colors = false } } }, "colors must be a table")
  assert_config_error({ powerline = { tabs = { colors = { unknown = "#000000" } } } }, "unknown")
  assert_config_error({ powerline = { tabs = { colors = { active_background = "" } } } }, "active_background")
  assert_config_error({ powerline = { status = {} }, status_bar = false }, "requires status_bar")
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

helper.test("file preview is a strict visibility boolean", function()
  local wezterm = helper.fake_wezterm()
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, { file_preview = true })

  assert_config_error({ file_preview = {} }, "file_preview")
  assert_config_error({ file_preview = "yes" }, "file_preview")
end)
