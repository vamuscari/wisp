package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

helper.test("refresh delegates cache invalidation to the wisp executable", function()
  local calls = {}
  local wezterm = helper.fake_wezterm {
    run_child_process = function(args)
      table.insert(calls, args)
      return true, "refreshed 2 projects\n", ""
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {
    config_file = "/Users/test/.config/wisp/config.toml",
  })
  local window = helper.fake_window()

  helper.run_callback(wisp.refresh_cache_action(), window, helper.fake_pane())

  helper.assert_table_equal(
    calls[1],
    { "/opt/bin/wisp", "--config", "/Users/test/.config/wisp/config.toml", "refresh" },
    "refresh command"
  )
  helper.assert_equal(#window.performed, 0, "refresh action count")
end)

helper.test("refresh logs child process failures", function()
  local wezterm = helper.fake_wezterm {
    run_child_process = function()
      return false, "", "wisp failed"
    end,
  }
  local wisp = helper.load_wezterm_adapter(wezterm)
  wisp.apply_to_config({}, {})

  helper.run_callback(wisp.refresh_cache_action(), helper.fake_window(), helper.fake_pane())

  helper.assert_equal(wezterm.logs[#wezterm.logs].level, "error", "refresh failure level")
  assert(wezterm.logs[#wezterm.logs].message:match "wisp failed", "refresh failure message")
end)
