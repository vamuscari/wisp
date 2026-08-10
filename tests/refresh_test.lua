package.path = "./?.lua;./?/init.lua;" .. package.path

local helper = require "tests.test_helper"

helper.test("refresh silently replaces and prewarms the discovery cache", function()
  helper.with_fake_time(100, function()
    local calls = {}
    local entries = {
      ["/Users/test/Repos"] = { "/Users/test/Repos/api" },
      ["/Users/test/Repos/api"] = { "/Users/test/Repos/api/src" },
      ["/Users/test/Artifacts"] = {},
    }
    local wezterm = helper.fake_wezterm {
      read_dir = function(path)
        calls[path] = (calls[path] or 0) + 1
        if not entries[path] then
          error("not a directory: " .. path)
        end
        return entries[path]
      end,
    }
    local wisp = helper.load_plugin(wezterm)
    wisp.apply_to_config({}, {
      roots = { "~/Repos" },
      projects = { { group = "Home", name = "Artifacts", path = "~/Artifacts" } },
      cache_ttl_seconds = 60,
    })

    local window = helper.fake_window()
    local pane = helper.fake_pane()
    helper.run_callback(wisp.project_picker_action(), window, pane)
    helper.assert_equal(calls["/Users/test/Repos"], 1, "initial root read")

    local action_count = #window.performed
    helper.run_callback(wisp.refresh_cache_action(), window, pane)
    helper.assert_equal(#window.performed, action_count, "refresh UI action count")
    helper.assert_equal(calls["/Users/test/Repos"], 2, "refreshed root read")
    helper.assert_equal(calls["/Users/test/Repos/api"], 2, "refreshed project read")
    helper.assert_equal(calls["/Users/test/Artifacts"], 2, "refreshed fixed project read")
    helper.assert_equal(calls["/Users/test/Repos/api/src"], nil, "deep directory remains lazy")

    helper.run_callback(wisp.project_picker_action(), window, pane)
    helper.assert_equal(calls["/Users/test/Repos"], 2, "picker reuses refreshed cache")
  end)
end)
